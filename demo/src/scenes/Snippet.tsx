import { AbsoluteFill, useCurrentFrame } from "remotion";
import { beats, easeInOut, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { spokenTokens } from "../lib/text.ts";
import { Caption, ClockChip, doubleTap, KeyCap, keyDepth } from "../overlay/Overlays.tsx";
import { Messages, SnippetChip } from "../screen/Apps.tsx";
import { Desktop } from "../screen/Desktop.tsx";
import { FlowPill } from "../screen/FlowPill.tsx";
import { PillDock, Spoken } from "../screen/Spoken.tsx";
import { sceneCues, sceneFrames } from "../timeline.ts";
import { World } from "../world/World.tsx";

const cue = sceneCues("snippet");

/** 12:15, café: a spoken trigger expands into a saved link. */
export function Snippet() {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { durationInFrames } = sceneFrames("snippet");
  const { presses, listenAt } = doubleTap(cue("tap"), cue("stop"));
  const push = ramp(frame, cue("push"), beats(1.5), easeInOut);
  const speaking = frame >= cue("speak") && frame < cue("stop") ? 1 : 0;

  return (
    <AbsoluteFill>
      <World
        place="cafe"
        time="noon"
        push={push}
        speaking={speaking}
        press={keyDepth(frame, presses)}
        screen={
          <Desktop
            time="noon"
            app={copy.snippet.app}
            clock={copy.menuClock.snippet}
            overlay={
              <>
                <Spoken
                  tokens={spokenTokens(copy.snippet.said, copy.lang)}
                  from={cue("speak")}
                  to={cue("stop") - 4}
                  hideAt={cue("paste")}
                />
                <PillDock>
                  <FlowPill listenAt={listenAt} stopAt={cue("stop")} doneAt={cue("paste")} />
                </PillDock>
                <SnippetChip at={cue("paste") + 8} until={durationInFrames} />
              </>
            }
          >
            <Messages pasteAt={cue("paste")} />
          </Desktop>
        }
      />
      <ClockChip text={copy.clock.snippet} start={0} end={cue("push") + 16} />
      <Caption text={copy.caption.snippet} start={cue("tap") - 10} end={durationInFrames + 8}>
        <KeyCap taps={presses} size={0.8} />
      </Caption>
    </AbsoluteFill>
  );
}
