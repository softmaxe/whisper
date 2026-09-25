import { AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig } from "remotion";
import { hash, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { graphemes } from "../lib/text.ts";
import { KeyCap } from "../overlay/Overlays.tsx";
import { FlowPill } from "../screen/FlowPill.tsx";
import { sceneCues } from "../timeline.ts";

const cue = sceneCues("intro");

function Bokeh() {
  const frame = useCurrentFrame();
  return (
    <>
      {Array.from({ length: 14 }, (_, i) => {
        const size = 80 + hash(i) * 220;
        const x = hash(i + 20) * 1920;
        const y = (hash(i + 40) * 1080 - frame * (0.3 + hash(i + 60) * 0.6)) % 1180;
        return (
          <div
            key={i}
            style={{
              position: "absolute",
              left: x - size / 2,
              top: ((y + 1180) % 1180) - 100,
              width: size,
              height: size,
              borderRadius: size,
              background: i % 3 === 0 ? "#fff" : i % 3 === 1 ? "#ffd1b8" : "#c9c3ff",
              opacity: 0.18,
              filter: "blur(6px)",
            }}
          />
        );
      })}
    </>
  );
}

/** The fn key, a Double tap, and the Recording pill coming alive. */
export function Intro() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const copy = useCopy();
  const taps = [cue("tap"), cue("tap") + 8];
  // The key is already on screen at frame 0, which GitHub shows as the poster.
  const keyIn =
    0.82 + 0.18 * spring({ frame: frame - cue("key"), fps, config: { damping: 14, mass: 0.8 } });
  const split = spring({ frame: frame - cue("pill"), fps, config: { damping: 18 } });
  const title = ramp(frame, cue("title"), 16);
  const characters = graphemes(copy.slogan);

  return (
    <AbsoluteFill
      style={{
        background: "linear-gradient(160deg, #ffe3cf 0%, #f6c6c8 45%, #b9b4ef 100%)",
        overflow: "hidden",
      }}
    >
      <Bokeh />
      <div
        style={{
          position: "absolute",
          left: "50%",
          top: interpolate(split, [0, 1], [540, 380]),
          transform: `translate(-50%, -50%) translateX(${interpolate(split, [0, 1], [0, -230])}px) scale(${keyIn})`,
        }}
      >
        <KeyCap taps={taps} size={1.8} />
      </div>
      <div
        style={{
          position: "absolute",
          left: "50%",
          top: 380,
          transform: `translate(-50%, -50%) translateX(${interpolate(split, [0, 1], [0, 150])}px)`,
          opacity: split,
        }}
      >
        <FlowPill listenAt={cue("pill")} stopAt={Infinity} doneAt={Infinity} scale={4.2} />
      </div>
      <div
        style={{
          position: "absolute",
          left: 0,
          right: 0,
          top: 590,
          textAlign: "center",
          fontFamily: copy.font,
          color: "#2b2733",
        }}
      >
        <div style={{ fontSize: 118, fontWeight: 800, letterSpacing: copy.lang === "en" ? -3 : 8 }}>
          {characters.map((character, i) => {
            const t = ramp(frame, cue("title") + i * 1.2, 12);
            return (
              <span
                key={i}
                style={{
                  display: "inline-block",
                  opacity: t,
                  transform: `translateY(${(1 - t) * 40}px)`,
                  whiteSpace: "pre",
                }}
              >
                {character}
              </span>
            );
          })}
        </div>
        <div
          style={{
            fontSize: 36,
            marginTop: 18,
            color: "#5a5068",
            opacity: ramp(frame, cue("title") + 14, 14),
            fontWeight: 500,
          }}
        >
          {copy.subtitle}
        </div>
      </div>
    </AbsoluteFill>
  );
}
