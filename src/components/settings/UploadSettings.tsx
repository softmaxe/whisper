import { useTranslation } from "react-i18next";
import { Network } from "../icons";
import { useSettingsStore } from "../../stores/settingsStore";
import { InferenceModeSelector } from "../ui/SettingsSection";
import SelfHostedPanel from "../SelfHostedPanel";

export function UploadTranscriptionPanel() {
  const { t } = useTranslation();
  const {
    setUploadTranscriptionMode,
    remoteTranscriptionUrl,
    setRemoteTranscriptionUrl,
    remoteTranscriptionModel,
    setRemoteTranscriptionModel,
  } = useSettingsStore();

  return (
    <div className="space-y-3">
      <InferenceModeSelector
        modes={[
          {
            id: "self-hosted",
            label: t("settingsPage.transcription.modes.selfHosted"),
            description: t("settingsPage.transcription.modes.selfHostedDesc"),
            icon: <Network className="w-4 h-4" />,
          },
        ]}
        activeMode="self-hosted"
        onSelect={setUploadTranscriptionMode}
      />
      <SelfHostedPanel
        service="transcription"
        url={remoteTranscriptionUrl}
        onUrlChange={setRemoteTranscriptionUrl}
        model={remoteTranscriptionModel}
        onModelChange={setRemoteTranscriptionModel}
      />
    </div>
  );
}
