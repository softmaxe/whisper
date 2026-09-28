import path from "node:path";
import { describe, expect, it } from "vitest";
import { ROOT } from "../scripts/paths.ts";
import { appSources } from "./film/global-setup.ts";

describe("Film test staleness sources", () => {
  it("include the app files the demo imports, followed through their own imports", () => {
    const app = appSources().map((file) => path.relative(path.join(ROOT, ".."), file));
    expect(app).toEqual(
      expect.arrayContaining([
        "src/components/dictation/waveformMath.ts",
        "src/helpers/voicePillPresentation.js",
        "src/helpers/voiceSurfaceGeometry.mjs",
        "src/helpers/voiceSurfaceGeometry.json",
      ]),
    );
    expect(app.every((file) => file.startsWith("src/"))).toBe(true);
  });
});
