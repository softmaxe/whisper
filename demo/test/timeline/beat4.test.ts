/**
 * Beat 4 · The day in review: night falls, History is searched and the match
 * highlighted, then Insights values are written and the activity bars drawn,
 * in story order, with picture and sound on the same times.
 */
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ROOT } from "../../scripts/paths.ts";
import { COPY } from "../../src/copy.ts";
import { BARS_DRAW_SECONDS, beat4 } from "../../timeline/beats/beat4-review.ts";
import { keystrokeTimes } from "../../timeline/helpers.ts";
import { LANGS } from "../../timeline/index.ts";

const m = beat4.moments;
const cuesOf = (type: string) => beat4.cues.filter((c) => c.type === type).map((c) => c.at);

describe("Beat 4 story", () => {
  it("captions History first, then Insights", () => {
    expect(beat4.captions.map((c) => c.id)).toEqual(["b4-history", "b4-insights"]);
    const [history, insights] = beat4.captions;
    expect(history.end).toBeGreaterThan(m.highlight + 1);
    expect(insights.start).toBeLessThanOrEqual(m.values[0]);
    expect(insights.end).toBeGreaterThan(m.today + 0.8);
  });

  it("falls to night, searches, filters, highlights, then writes values and draws the bars", () => {
    const [typing] = beat4.typing;
    const order = [
      m.nightfall,
      m.sketchIn,
      m.searchOpen,
      typing.start,
      typing.end,
      m.filter,
      m.highlight,
      m.insights,
      ...m.values,
      m.bars,
      m.bars + BARS_DRAW_SECONDS,
      m.today,
      m.beatOut,
    ];
    expect([...order].sort((a, b) => a - b)).toEqual(order);
  });

  it("writes one value per Insights card", () => {
    expect(m.values).toHaveLength(4);
  });

  it("finds the query in today's History in both cuts", () => {
    for (const lang of LANGS) {
      const { query, history } = COPY[lang].whisper;
      const matches = history.filter((e) => e.text.includes(query));
      expect(matches.length, lang).toBeGreaterThan(0);
      expect(matches.length, lang).toBeLessThan(history.length);
    }
  });
});

describe("Beat 4 sound", () => {
  it("clicks a key on every keystroke of the query", () => {
    const [typing] = beat4.typing;
    expect(cuesOf("key_click")).toEqual(keystrokeTimes(typing));
  });

  it("scratches the pen for the highlight, each value, the bars and today's circle", () => {
    expect(cuesOf("pen_scratch")).toEqual([m.highlight, ...m.values, m.bars, m.today]);
  });
});

describe("Beat 4 text", () => {
  it("writes the values and the today note in characters of the bundled font subset", () => {
    const charset = new Set(fs.readFileSync(path.join(ROOT, "public/fonts/LXGWWenKai-Regular.subset.charset.txt"), "utf8"));
    for (const lang of LANGS) {
      const { insights, today } = COPY[lang].whisper;
      const v = insights.values;
      const handwritten = [
        v.words.toLocaleString(lang),
        insights.days(v.streak),
        v.wpm.toLocaleString(lang),
        v.dictations.toLocaleString(lang),
        today,
      ].join("");
      expect([...handwritten].filter((ch) => ch !== " " && !charset.has(ch)), lang).toEqual([]);
    }
  });
});

describe("Review frames", () => {
  it("catch the History highlight once drawn, and the Insights chart once today is circled", () => {
    const at = Object.fromEntries((beat4.reviewFrames ?? []).map((f) => [f.name, f.at]));
    expect(at["history-highlight"]).toBeGreaterThan(m.highlight + 0.8);
    expect(at["history-highlight"]).toBeLessThan(m.insights);
    expect(at["insights-chart"]).toBeGreaterThan(m.today + 0.8);
    expect(at["insights-chart"]).toBeLessThan(m.beatOut);
  });
});
