import { AbsoluteFill, Img, spring, staticFile, useCurrentFrame, useVideoConfig } from "remotion";
import { beats, easeInOut, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { revealText } from "../lib/text.ts";
import { Desktop } from "../screen/Desktop.tsx";
import { InsightsPage, WhisperShell } from "../screen/WhisperApp.tsx";
import { MONO } from "../theme.ts";
import { cueFrame } from "../timeline.ts";
import { World } from "../world/World.tsx";

const cue = (name: Parameters<typeof cueFrame<"outro">>[1]) => cueFrame("outro", name);

/** The camera pulls back, the day is done, and the install command types out. */
export function Outro() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const copy = useCopy();
  const pull = 1 - ramp(frame, cue("pull"), beats(1.4), easeInOut);
  const stretch = ramp(frame, 12, 26, easeInOut);
  const card = ramp(frame, cue("logo"), 14);
  const icon = spring({ frame: frame - cue("logo"), fps, config: { damping: 13 } });
  const command = revealText(
    copy.outro.install,
    ramp(frame, cue("install"), 30, (t) => t)
  );
  const link = ramp(frame, cue("link"), 14);

  return (
    <AbsoluteFill>
      <World
        place="bedroom"
        time="night"
        push={pull}
        stretch={stretch}
        screen={
          <Desktop time="night" app="Whisper" clock={copy.menuClock.night}>
            <WhisperShell page="insights" title={copy.whisper.nav.insights}>
              <InsightsPage countAt={-200} />
            </WhisperShell>
          </Desktop>
        }
      />
      <AbsoluteFill
        style={{
          background: `radial-gradient(circle at 50% 40%, rgba(40,44,110,${0.82 * card}), rgba(10,11,30,${0.94 * card}))`,
          backdropFilter: card > 0 ? `blur(${card * 10}px)` : undefined,
        }}
      />
      {card > 0 && (
        <AbsoluteFill
          style={{
            alignItems: "center",
            justifyContent: "center",
            fontFamily: copy.font,
            color: "#fff",
          }}
        >
          <Img
            src={staticFile("icon.png")}
            style={{
              width: 200,
              height: 200,
              transform: `scale(${icon})`,
              filter: "drop-shadow(0 24px 40px rgba(0,0,0,0.5))",
            }}
          />
          <div
            style={{
              fontSize: 96,
              fontWeight: 800,
              marginTop: 18,
              opacity: card,
              letterSpacing: -2,
            }}
          >
            Whisper
          </div>
          <div style={{ fontSize: 40, marginTop: 6, color: "#c9cdf5", opacity: card }}>
            {copy.slogan}
          </div>
          <div
            style={{
              marginTop: 48,
              minWidth: 900,
              padding: "22px 30px",
              borderRadius: 18,
              background: "rgba(0,0,0,0.45)",
              border: "1px solid rgba(255,255,255,0.12)",
              fontFamily: MONO,
              fontSize: 32,
              opacity: ramp(frame, cue("install") - 6, 8),
            }}
          >
            <span style={{ color: "#7fbf7f" }}>$ </span>
            {command}
            <span
              style={{
                display: "inline-block",
                width: 14,
                height: 32,
                background: "#9fb8ff",
                marginLeft: 4,
                verticalAlign: -4,
                opacity: Math.floor(frame / 12) % 2,
              }}
            />
          </div>
          <div
            style={{
              marginTop: 28,
              fontSize: 30,
              color: "#9fb8ff",
              opacity: link,
              fontFamily: MONO,
            }}
          >
            {copy.outro.link}
          </div>
        </AbsoluteFill>
      )}
    </AbsoluteFill>
  );
}
