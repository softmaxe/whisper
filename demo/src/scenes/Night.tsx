import { AbsoluteFill, useCurrentFrame } from "remotion";
import { beats, easeInOut, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { Caption, ClockChip } from "../overlay/Overlays.tsx";
import { Desktop } from "../screen/Desktop.tsx";
import { HistoryPage, InsightsPage, WhisperShell } from "../screen/WhisperApp.tsx";
import { cueFrame, sceneFrames } from "../timeline.ts";
import { World } from "../world/World.tsx";

const cue = (name: Parameters<typeof cueFrame<"night">>[1]) => cueFrame("night", name);

/** 10:20, bedroom: the day in History, found with ⌘K, then Insights. */
export function Night() {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { durationInFrames } = sceneFrames("night");
  const push = ramp(frame, cue("push"), beats(1.5), easeInOut);
  const onInsights = frame >= cue("insights");

  return (
    <AbsoluteFill>
      <World
        place="bedroom"
        time="night"
        push={push}
        screen={
          <Desktop time="night" app="Whisper" clock={copy.menuClock.night}>
            <WhisperShell
              page={onInsights ? "insights" : "home"}
              title={onInsights ? copy.whisper.nav.insights : copy.whisper.nav.home}
            >
              {onInsights ? (
                <InsightsPage countAt={cue("count")} />
              ) : (
                <HistoryPage
                  searchAt={cue("search")}
                  copyAt={cue("copy")}
                  closeAt={cue("insights") - 10}
                />
              )}
            </WhisperShell>
          </Desktop>
        }
      />
      <ClockChip text={copy.clock.night} start={0} end={cue("push") + 16} night />
      <Caption text={copy.caption.history} start={cue("search") - 12} end={cue("insights")} />
      <Caption text={copy.caption.insights} start={cue("insights")} end={durationInFrames + 8} />
    </AbsoluteFill>
  );
}
