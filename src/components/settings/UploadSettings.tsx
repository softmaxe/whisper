import { useSettingsStore } from "../../stores/settingsStore";
import SelfHostedPanel from "../SelfHostedPanel";

export function UploadTranscriptionPanel() {
  const {
    remoteTranscriptionUrl,
    setRemoteTranscriptionUrl,
    remoteTranscriptionModel,
    setRemoteTranscriptionModel,
  } = useSettingsStore();

  return (
    <SelfHostedPanel
      service="transcription"
      url={remoteTranscriptionUrl}
      onUrlChange={setRemoteTranscriptionUrl}
      model={remoteTranscriptionModel}
      onModelChange={setRemoteTranscriptionModel}
    />
  );
}
