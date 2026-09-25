import { AbsoluteFill, useCurrentFrame } from "remotion";
import { useCopy } from "../lib/copy-context.tsx";
import { spokenTokens } from "../lib/text.ts";
import { Caption, ClockChip, KeyCap, keyDepth } from "../overlay/Overlays.tsx";
import { Editor } from "../screen/Apps.tsx";
import { Desktop } from "../screen/Desktop.tsx";
import { FlowPill } from "../screen/FlowPill.tsx";
import { PillDock, Spoken } from "../screen/Spoken.tsx";
import { sceneCues, sceneFrames } from "../timeline.ts";
import { World } from "../world/World.tsx";

const cue = sceneCues("hold");

/** 3:05, back at the office: Hold mode drops a comment into code. */
export function Hold() {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { durationInFrames } = sceneFrames("hold");
  const held = frame >= cue("press") && frame < cue("release");
  const comment = copy.hold.comment.trim().replace(/^\/\/\s*/, "");

  return (
    <AbsoluteFill>
      <World
        place="office"
        time="afternoon"
        push={1}
        press={keyDepth(frame, [cue("press")], cue("release"))}
        screen={
          <Desktop
            time="afternoon"
            app={copy.lang === "en" ? "Code" : "代码"}
            clock={copy.menuClock.hold}
            overlay={
              <>
                <Spoken
                  tokens={spokenTokens(comment, copy.lang)}
                  from={cue("speak")}
                  to={cue("release") - 6}
                  hideAt={cue("paste")}
                />
                <PillDock>
                  <FlowPill
                    listenAt={cue("press") + 2}
                    stopAt={cue("release")}
                    doneAt={cue("paste")}
                    held={held}
                  />
                </PillDock>
              </>
            }
          >
            <Editor pasteAt={cue("paste")} />
          </Desktop>
        }
      />
      <ClockChip text={copy.clock.hold} start={0} end={cue("speak") + 20} />
      <Caption text={copy.caption.hold} kicker={copy.key.hold} start={0} end={durationInFrames + 8}>
        <KeyCap taps={[cue("press")]} release={cue("release")} size={0.8} />
      </Caption>
    </AbsoluteFill>
  );
}
