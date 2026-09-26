import { AbsoluteFill, useCurrentFrame } from "remotion";
import { beats, easeInOut, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { Caption, ClockChip, doubleTap, KeyCap, keyDepth } from "../overlay/Overlays.tsx";
import { Desktop } from "../screen/Desktop.tsx";
import { FlowPill } from "../screen/FlowPill.tsx";
import { Mail } from "../screen/Mail.tsx";
import { PillDock, Spoken } from "../screen/Spoken.tsx";
import { sceneCues, sceneFrames } from "../timeline.ts";
import { World } from "../world/World.tsx";

const cue = sceneCues("morning");

/** 7:45, kitchen: the first Double tap, a reply email, and text cleanup. */
export function Morning() {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { durationInFrames } = sceneFrames("morning");
  const { presses, listenAt } = doubleTap(cue("tap"), cue("stop"));
  const push = ramp(frame, cue("push"), beats(2), easeInOut);
  const speaking = frame >= cue("speak") && frame < cue("stop") ? 1 : 0;

  return (
    <AbsoluteFill>
      <World
        place="kitchen"
        time="dawn"
        push={push}
        speaking={speaking}
        press={keyDepth(frame, presses)}
        screen={
          <Desktop
            time="dawn"
            app={copy.mail.app}
            clock={copy.menuClock.morning}
            overlay={
              <>
                <Spoken
                  tokens={copy.mail.spoken}
                  from={cue("speak")}
                  to={cue("stop") - 4}
                  strikeAt={cue("stop") + 2}
                  hideAt={cue("paste") + Math.round(beats(3.6))}
                />
                <PillDock>
                  <FlowPill listenAt={listenAt} stopAt={cue("stop")} doneAt={cue("paste")} />
                </PillDock>
              </>
            }
          >
            <Mail pasteAt={cue("paste")} />
          </Desktop>
        }
      />
      <ClockChip text={copy.clock.morning} start={4} end={cue("push") + 20} />
      <Caption text={copy.caption.dictate} start={cue("tap") - 12} end={cue("cleanup")}>
        <KeyCap taps={presses} size={0.8} />
      </Caption>
      <Caption text={copy.caption.cleanup} start={cue("cleanup")} end={durationInFrames + 8} />
    </AbsoluteFill>
  );
}
