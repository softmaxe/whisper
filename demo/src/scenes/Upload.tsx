import { AbsoluteFill, useCurrentFrame } from "remotion";
import { beats, easeInOut, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { Caption, ClockChip } from "../overlay/Overlays.tsx";
import { Desktop } from "../screen/Desktop.tsx";
import { DraggedFiles, UploadPage, WhisperShell } from "../screen/WhisperApp.tsx";
import { sceneCues, sceneFrames } from "../timeline.ts";
import { World } from "../world/World.tsx";

const cue = sceneCues("upload");

/** 6:40, living room: the day's recordings transcribe in a batch. */
export function Upload() {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { durationInFrames } = sceneFrames("upload");
  const push = ramp(frame, cue("push"), beats(1.5), easeInOut);

  return (
    <AbsoluteFill>
      <World
        place="living"
        time="evening"
        push={push}
        screen={
          <Desktop
            time="evening"
            app="Whisper"
            clock={copy.menuClock.upload}
            overlay={<DraggedFiles dropAt={cue("drop")} />}
          >
            <WhisperShell page="upload" title={copy.whisper.nav.upload}>
              <UploadPage dropAt={cue("drop")} progressAt={cue("progress")} doneAt={cue("done")} />
            </WhisperShell>
          </Desktop>
        }
      />
      <ClockChip text={copy.clock.upload} start={0} end={cue("push") + 16} />
      <Caption text={copy.caption.upload} start={cue("drop") - 10} end={durationInFrames + 8} />
    </AbsoluteFill>
  );
}
