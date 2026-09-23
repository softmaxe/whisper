import { useTranslation } from "react-i18next";
import { useSettingsStore } from "../../stores/settingsStore";
import OpenAICompatiblePanel from "../OpenAICompatiblePanel";
import { Toggle } from "../ui/toggle";

// Server, key, model, and thinking controls for the self-hosted cleanup model.
export default function InferenceConfigEditor() {
  const { t } = useTranslation();
  const cleanupRemoteUrl = useSettingsStore((s) => s.cleanupRemoteUrl);
  const cleanupCustomApiKey = useSettingsStore((s) => s.cleanupCustomApiKey);
  const cleanupModel = useSettingsStore((s) => s.cleanupModel);
  const cleanupDisableThinking = useSettingsStore((s) => s.cleanupDisableThinking);
  const setCleanupRemoteUrl = useSettingsStore((s) => s.setCleanupRemoteUrl);
  const setCleanupCustomApiKey = useSettingsStore((s) => s.setCleanupCustomApiKey);
  const setCleanupModel = useSettingsStore((s) => s.setCleanupModel);
  const setCleanupDisableThinking = useSettingsStore((s) => s.setCleanupDisableThinking);

  return (
    <div className="space-y-3">
      <OpenAICompatiblePanel
        baseUrl={cleanupRemoteUrl}
        setBaseUrl={setCleanupRemoteUrl}
        apiKey={cleanupCustomApiKey}
        setApiKey={setCleanupCustomApiKey}
        model={cleanupModel}
        setModel={setCleanupModel}
        baseUrlPlaceholder="http://192.168.1.126:11434/v1"
        helpExamples={
          <p className="text-xs text-muted-foreground">{t("reasoning.selfHosted.endpointHelp")}</p>
        }
      />

      <div className="flex items-start justify-between gap-3 pt-1">
        <div className="flex-1 min-w-0">
          <h4 className="text-sm font-medium text-foreground">
            {t("reasoning.disableThinking.label")}
          </h4>
          <p className="text-xs text-muted-foreground">{t("reasoning.disableThinking.help")}</p>
        </div>
        <Toggle checked={cleanupDisableThinking} onChange={setCleanupDisableThinking} />
      </div>
    </div>
  );
}
