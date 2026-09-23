import { getSettings } from "../stores/settingsStore";

// Single and batch uploads use the existing History write and retention choice.
// An unsaved successful result remains available to copy in the upload view.
export async function saveUploadTranscription(
  text: string
): Promise<{ success: boolean; id: number | null }> {
  if (!getSettings().dataRetentionEnabled) {
    return { success: true, id: null };
  }
  return window.electronAPI.saveTranscription(text, null, { routeKind: "upload" });
}
