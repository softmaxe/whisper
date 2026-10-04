import { useSettingsStore } from "../stores/settingsStore";
import { getDefaultHotkey } from "../utils/hotkeys";

// Prefer the hotkeys the main process actually registered over the stored
// preference (they diverge on partial registration and DE-native backends).
export const useHotkey = () =>
  useSettingsStore((s) => s.activeDictationKey || s.dictationKey) || getDefaultHotkey();
