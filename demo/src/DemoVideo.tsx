import { AbsoluteFill, Freeze, Html5Audio, Sequence, staticFile, useCurrentFrame } from "remotion";
import type { JSX, ReactNode } from "react";
import { COPY, type Lang } from "./copy.ts";
import { ramp } from "./lib/anim.ts";
import { CopyContext } from "./lib/copy-context.tsx";
import { Chat } from "./scenes/Chat.tsx";
import { Hold } from "./scenes/Hold.tsx";
import { Intro } from "./scenes/Intro.tsx";
import { Morning } from "./scenes/Morning.tsx";
import { Night } from "./scenes/Night.tsx";
import { Outro } from "./scenes/Outro.tsx";
import { Settings } from "./scenes/Settings.tsx";
import { Snippet } from "./scenes/Snippet.tsx";
import { Upload } from "./scenes/Upload.tsx";
import { FPS, MUSIC_OFFSET_SECONDS, SCENES, sceneFrames, type SceneId } from "./timeline.ts";

const SCENE_COMPONENTS: Record<SceneId, () => JSX.Element> = {
  intro: Intro,
  morning: Morning,
  chat: Chat,
  snippet: Snippet,
  hold: Hold,
  upload: Upload,
  night: Night,
  settings: Settings,
  outro: Outro,
};

/** Frames each scene fades in over the end of the one before it. */
const CROSSFADE = 8;

/** Holds the scene's first frame while it fades in over the previous scene. */
function FadeIn({ children }: { children: ReactNode }) {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{ opacity: ramp(frame, 0, CROSSFADE, (t) => t) }}>
      <Freeze frame={CROSSFADE} active={(f) => f < CROSSFADE}>
        <Sequence from={CROSSFADE}>{children}</Sequence>
      </Freeze>
    </AbsoluteFill>
  );
}

export function DemoVideo({ lang }: { lang: Lang }) {
  const copy = COPY[lang];
  return (
    <CopyContext.Provider value={copy}>
      <AbsoluteFill style={{ background: "#111", fontFamily: copy.font }}>
        {SCENES.map(({ id }, index) => {
          const Scene = SCENE_COMPONENTS[id];
          const { from, durationInFrames } = sceneFrames(id);
          if (index === 0) {
            return (
              <Sequence key={id} from={from} durationInFrames={durationInFrames} name={id}>
                <Scene />
              </Sequence>
            );
          }
          return (
            <Sequence
              key={id}
              from={from - CROSSFADE}
              durationInFrames={durationInFrames + CROSSFADE}
              name={id}
            >
              <FadeIn>
                <Scene />
              </FadeIn>
            </Sequence>
          );
        })}
        <Html5Audio
          src={staticFile("music.wav")}
          trimBefore={Math.max(0, Math.round(MUSIC_OFFSET_SECONDS * FPS))}
        />
      </AbsoluteFill>
    </CopyContext.Provider>
  );
}
