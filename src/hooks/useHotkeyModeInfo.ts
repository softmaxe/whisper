import { useEffect, useState } from "react";
import logger from "../utils/logger";

export interface HotkeyModeInfo {
  supportsPushToTalk: boolean;
  pushToTalkUnavailableReason: string | null;
  /** False until main has answered; the defaults above are optimistic placeholders. */
  loaded: boolean;
}

const DEFAULT_INFO: HotkeyModeInfo = {
  supportsPushToTalk: true,
  pushToTalkUnavailableReason: null,
  loaded: false,
};

/**
 * Resolves whether the dictation hotkey supports push-to-talk. `scope` tags log
 * output for the calling surface.
 */
export function useHotkeyModeInfo(scope: string, hotkey?: string): HotkeyModeInfo {
  const [modeInfo, setModeInfo] = useState<HotkeyModeInfo>(DEFAULT_INFO);

  useEffect(() => {
    let cancelled = false;
    const checkHotkeyMode = async () => {
      try {
        const info = await window.electronAPI?.getHotkeyModeInfo?.(hotkey);
        if (!info || cancelled) return;
        setModeInfo({
          supportsPushToTalk: info.supportsPushToTalk,
          pushToTalkUnavailableReason: info.pushToTalkUnavailableReason,
          loaded: true,
        });
      } catch (error) {
        logger.error("Failed to check hotkey mode", { error }, scope);
      }
    };
    checkHotkeyMode();
    return () => {
      cancelled = true;
    };
  }, [scope, hotkey]);

  return modeInfo;
}
