import { Composition } from "remotion";
import { DemoVideo } from "./DemoVideo.tsx";
import { FPS, HEIGHT, TOTAL_FRAMES, WIDTH } from "./timeline.ts";
import "./fonts.css";

export function Root() {
  return (
    <>
      <Composition
        id="WhisperDemo-en"
        component={DemoVideo}
        durationInFrames={TOTAL_FRAMES}
        fps={FPS}
        width={WIDTH}
        height={HEIGHT}
        defaultProps={{ lang: "en" as const }}
      />
      <Composition
        id="WhisperDemo-zh"
        component={DemoVideo}
        durationInFrames={TOTAL_FRAMES}
        fps={FPS}
        width={WIDTH}
        height={HEIGHT}
        defaultProps={{ lang: "zh-CN" as const }}
      />
    </>
  );
}
