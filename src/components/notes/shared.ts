import { cn } from "../lib/utils";
import type { FolderItem } from "../../types/electron";

export const DEFAULT_FOLDER_NAME = "Personal";
export const MEETINGS_FOLDER_NAME = "Meetings";
export const VIDEOS_FOLDER_NAME = "Videos";

const DEFAULT_FOLDER_LABEL_KEYS: Record<string, string> = {
  [DEFAULT_FOLDER_NAME]: "notes.folders.defaults.personal",
  [MEETINGS_FOLDER_NAME]: "notes.folders.defaults.meetings",
  [VIDEOS_FOLDER_NAME]: "notes.folders.defaults.videos",
};

export function defaultFolderDisplayName(
  folder: Pick<FolderItem, "name" | "is_default">,
  t: (key: string) => string
): string {
  if (!folder.is_default) return folder.name;
  const key = DEFAULT_FOLDER_LABEL_KEYS[folder.name];
  return key ? t(key) : folder.name;
}

// Transcription error codes → notes.upload.* i18n keys. Codes absent here fall
// back to the raw main-process message.
const TRANSCRIPTION_ERROR_KEYS: Record<string, string> = {
  CUSTOM_ENDPOINT_INVALID: "customEndpointInvalid",
};

// Call sites pass either a returned result or a caught error, so the key is
// resolved from whichever shape they hold.
export function transcriptionErrorKey(failure: unknown): string | undefined {
  const code = (failure as { code?: string } | null | undefined)?.code;
  return code ? TRANSCRIPTION_ERROR_KEYS[code] : undefined;
}
