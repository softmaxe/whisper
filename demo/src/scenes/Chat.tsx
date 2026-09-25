import { AbsoluteFill, useCurrentFrame } from "remotion";
import { beats, easeInOut, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { spokenTokens } from "../lib/text.ts";
import { Caption, ClockChip, KeyCap, keyDepth } from "../overlay/Overlays.tsx";
import { Chat as ChatWindow, LearnedToast } from "../screen/Apps.tsx";
import { Desktop } from "../screen/Desktop.tsx";
import { FlowPill } from "../screen/FlowPill.tsx";
import { PillDock, Spoken } from "../screen/Spoken.tsx";
import { cueFrame, sceneFrames } from "../timeline.ts";
import { World } from "../world/World.tsx";

const cue = (name: Parameters<typeof cueFrame<"chat">>[1]) => cueFrame("chat", name);

/** 9:30, office: a misheard name is corrected once and learned. */
export function Chat() {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { durationInFrames } = sceneFrames("chat");
  const taps = [cue("tap"), cue("tap") + Math.round(beats(0.35))];
  const stopTap = cue("stop") - 2;
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
        press={keyDepth(frame, [...taps, stopTap])}
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
                  <FlowPill listenAt={taps[1] + 2} stopAt={cue("stop")} doneAt={cue("paste")} />
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
        <KeyCap taps={[...taps, stopTap]} size={0.8} />
      </Caption>
    </AbsoluteFill>
  );
}
