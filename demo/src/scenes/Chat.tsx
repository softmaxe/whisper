import { AbsoluteFill, useCurrentFrame } from "remotion";
import { beats, easeInOut, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { spokenTokens } from "../lib/text.ts";
import { Caption, ClockChip, doubleTap, KeyCap, keyDepth } from "../overlay/Overlays.tsx";
import { Chat as ChatWindow, LearnedToast } from "../screen/Apps.tsx";
import { Desktop } from "../screen/Desktop.tsx";
import { FlowPill } from "../screen/FlowPill.tsx";
import { PillDock, Spoken } from "../screen/Spoken.tsx";
import { sceneCues, sceneFrames } from "../timeline.ts";
import { World } from "../world/World.tsx";

const cue = sceneCues("chat");

/** 9:30, office: a misheard name is corrected once and learned. */
export function Chat() {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { durationInFrames } = sceneFrames("chat");
  const { presses, listenAt } = doubleTap(cue("tap"), cue("stop"));
  const fixAt = cue("fix") + 16;
  const sendAt = cue("learn") + 20;
  const push = ramp(frame, cue("push"), beats(1.5), easeInOut);
  const speaking = frame >= cue("speak") && frame < cue("stop") ? 1 : 0;

  return (
    <AbsoluteFill>
      <World
        place="office"
        time="morning"
        push={push}
        speaking={speaking}
        press={keyDepth(frame, presses)}
        screen={
          <Desktop
            time="morning"
            app={copy.chat.app}
            clock={copy.menuClock.chat}
            overlay={
              <>
                <Spoken
                  tokens={spokenTokens(copy.chat.heard, copy.lang)}
                  from={cue("speak")}
                  to={cue("stop") - 4}
                  hideAt={cue("paste")}
                />
                <PillDock>
                  <FlowPill listenAt={listenAt} stopAt={cue("stop")} doneAt={cue("paste")} />
                </PillDock>
                <LearnedToast at={cue("learn")} until={durationInFrames - 4} />
              </>
            }
          >
            <ChatWindow
              pasteAt={cue("paste")}
              selectAt={cue("fix")}
              fixAt={fixAt}
              sendAt={sendAt}
            />
          </Desktop>
        }
      />
      <ClockChip text={copy.clock.chat} start={0} end={cue("push") + 16} />
      <Caption text={copy.caption.learn} start={cue("tap") - 10} end={durationInFrames + 8}>
        <KeyCap taps={presses} size={0.8} />
      </Caption>
    </AbsoluteFill>
  );
}
