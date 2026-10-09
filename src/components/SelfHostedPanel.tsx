import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "./ui/input";
import { SettingsField, SettingsPanel, SettingsPanelRow } from "./ui/SettingsSection";

interface SelfHostedPanelProps {
  service: "transcription" | "reasoning";
  url: string;
  onUrlChange: (url: string) => void;
  model?: string;
  onModelChange?: (model: string) => void;
}

export default function SelfHostedPanel({
  service,
  url,
  onUrlChange,
  model,
  onModelChange,
}: SelfHostedPanelProps) {
  const { t } = useTranslation();
  const fieldId = useId();

  const placeholderUrl =
    service === "transcription" ? "http://192.168.1.126:8178" : "http://192.168.1.126:8080";

  return (
    <SettingsPanel>
      <SettingsPanelRow>
        <SettingsField label={t("settingsPage.selfHosted.serverUrl")} htmlFor={`${fieldId}-url`}>
          <Input
            id={`${fieldId}-url`}
            dir="ltr"
            value={url}
            onChange={(e) => onUrlChange(e.target.value)}
            placeholder={placeholderUrl}
            className="h-8 text-[13px]"
          />
        </SettingsField>
      </SettingsPanelRow>
      {onModelChange && (
        <SettingsPanelRow>
          <SettingsField label={t("common.model")} htmlFor={`${fieldId}-model`}>
            <Input
              id={`${fieldId}-model`}
              dir="ltr"
              value={model ?? ""}
              onChange={(e) => onModelChange(e.target.value)}
              placeholder="Whisper-Large-v3-Turbo"
              className="h-8 text-[13px]"
            />
          </SettingsField>
        </SettingsPanelRow>
      )}
    </SettingsPanel>
  );
}
