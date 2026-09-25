import { AbsoluteFill, useCurrentFrame } from "remotion";
import { ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { Desktop } from "../screen/Desktop.tsx";
import { SettingsPage, WhisperShell } from "../screen/WhisperApp.tsx";
import { cueFrame } from "../timeline.ts";
import { World } from "../world/World.tsx";

const cue = (name: Parameters<typeof cueFrame<"settings">>[1]) => cueFrame("settings", name);

/** A glimpse of Settings: the speech and cleanup servers are the user's own. */
export function Settings() {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const tagline = ramp(frame, cue("tagline"), 12);
  return (
    <AbsoluteFill>
      <World
        place="bedroom"
        time="night"
        push={1}
        screen={
          <Desktop time="night" app="Whisper" clock={copy.menuClock.night}>
            <WhisperShell page="settings" title={copy.whisper.nav.settings}>
              <SettingsPage asrAt={cue("asr")} cleanupAt={cue("cleanup")} />
            </WhisperShell>
          </Desktop>
        }
      />
      <AbsoluteFill
        style={{
          background: `linear-gradient(0deg, rgba(10,11,30,${0.85 * tagline}) 0%, rgba(10,11,30,${0.35 * tagline}) 45%, transparent 70%)`,
        }}
      />
      <div
        style={{
          position: "absolute",
          left: 0,
          right: 0,
          bottom: 90,
          textAlign: "center",
          color: "#fff",
          fontFamily: copy.font,
          opacity: tagline,
          transform: `translateY(${(1 - tagline) * 24}px)`,
        }}
      >
        <div
          style={{ fontSize: 76, fontWeight: 800, letterSpacing: copy.lang === "en" ? -1.5 : 4 }}
        >
          {copy.caption.servers}
        </div>
        <div style={{ fontSize: 30, marginTop: 14, color: "#c9cdf5" }}>
          {copy.caption.serversNote}
        </div>
      </div>
    </AbsoluteFill>
  );
}
