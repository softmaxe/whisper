import { useEffect, useState } from "react";
import { VOICE_PILL_FINISH_MS } from "../helpers/voicePillPresentation";

/** Keep only the presentation alive while a completed floating pill fades out. */
export function usePillFinish(isActive, hasFeedback) {
  const [previousActive, setPreviousActive] = useState(isActive);
  const [finishing, setFinishing] = useState(false);

  // Update during render so the window owner never sees an idle frame between
  // the active session and its exit. Recovery and new requests take over now.
  if (previousActive !== isActive) {
    setPreviousActive(isActive);
    setFinishing(previousActive && !isActive && !hasFeedback);
  } else if (hasFeedback && finishing) {
    setFinishing(false);
  }

  useEffect(() => {
    if (!finishing) return undefined;
    let cancelled = false;
    const timer = setTimeout(() => {
      if (!cancelled) setFinishing(false);
    }, VOICE_PILL_FINISH_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [finishing]);

  return finishing && !isActive && !hasFeedback;
}
