import { describe, expect, it } from "vitest";
import {
  BEATS_PER_BAR,
  FPS,
  SCENES,
  TOTAL_BEATS,
  TOTAL_FRAMES,
  beatToFrame,
  cueBeat,
  sceneFrames,
} from "../src/timeline.ts";

describe("timeline", () => {
  it("fills one minute exactly", () => {
    expect(TOTAL_FRAMES).toBe(60 * FPS);
    expect(beatToFrame(TOTAL_BEATS)).toBe(TOTAL_FRAMES);
  });

  it("cuts only on strong beats so picture changes land with the music", () => {
    let beat = 0;
    for (const scene of SCENES) {
      expect(beat % (BEATS_PER_BAR / 2)).toBe(0);
      beat += scene.beats;
    }
    expect(beat).toBe(TOTAL_BEATS);
  });

  it("lays scenes end to end without gaps", () => {
    const ranges = SCENES.map((scene) => sceneFrames(scene.id));
    expect(ranges[0].from).toBe(0);
    for (let i = 1; i < ranges.length; i += 1) {
      expect(ranges[i].from).toBe(ranges[i - 1].from + ranges[i - 1].durationInFrames);
    }
    const last = ranges[ranges.length - 1];
    expect(last.from + last.durationInFrames).toBe(TOTAL_FRAMES);
  });

  it("keeps every cue inside its scene", () => {
    for (const scene of SCENES) {
      for (const beat of Object.values(scene.cues)) {
        expect(beat).toBeGreaterThanOrEqual(0);
        expect(beat).toBeLessThan(scene.beats);
      }
    }
  });

  it("resolves cues to absolute beats", () => {
    const morning = SCENES.find((scene) => scene.id === "morning")!;
    const start = SCENES.slice(0, SCENES.indexOf(morning)).reduce((sum, s) => sum + s.beats, 0);
    expect(cueBeat("morning", "paste")).toBe(start + morning.cues.paste);
  });
});
