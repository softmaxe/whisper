import { useTranslation } from "react-i18next";
import { useSettingsStore } from "../../stores/settingsStore";
import OpenAICompatiblePanel from "../OpenAICompatiblePanel";
import { SectionHeader, SettingsPanel, SettingsPanelRow, SettingsRow } from "../ui/SettingsSection";
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
    <div>
      <SectionHeader
        title={t("settingsPage.llms.server.title")}
        description={t("settingsPage.llms.server.description")}
      />
      <SettingsPanel>
        <OpenAICompatiblePanel
          baseUrl={cleanupRemoteUrl}
          setBaseUrl={setCleanupRemoteUrl}
          apiKey={cleanupCustomApiKey}
          setApiKey={setCleanupCustomApiKey}
          model={cleanupModel}
          setModel={setCleanupModel}
          baseUrlPlaceholder="http://192.168.1.126:11434/v1"
          helpExamples={t("reasoning.selfHosted.endpointHelp")}
        />
        <SettingsPanelRow>
          <SettingsRow
            label={t("reasoning.disableThinking.label")}
            description={t("reasoning.disableThinking.help")}
          >
            <Toggle checked={cleanupDisableThinking} onChange={setCleanupDisableThinking} />
          </SettingsRow>
        </SettingsPanelRow>
      </SettingsPanel>
    </div>
  );
}
