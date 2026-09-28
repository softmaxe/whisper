/**
 * Beat 5 · Your servers: the house with the Mac and servers, the data kept
 * inside it, then the logo, install command and link, and a blank ending.
 */
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ROOT } from "../../scripts/paths.ts";
import { COPY } from "../../src/copy.ts";
import { BEAT5_DURATIONS, BEAT5_TEXT, beat5 } from "../../timeline/beats/beat5-servers.ts";
import { FILM, LANGS } from "../../timeline/index.ts";

const m = beat5.moments;
const d = BEAT5_DURATIONS;
const cuesOf = (type: string) => beat5.cues.filter((c) => c.type === type).map((c) => c.at);
/** When the Film's closing fade starts: from here on the picture must already be blank paper. */
const LAST_SECOND = FILM.durationSeconds - FILM.fadeOutSeconds;

describe("Beat 5 story", () => {
  it("builds the house, fills it, joins it with data arrows, then runs a Dictation inside", () => {
    const order = [m.sketchIn, m.mac, ...m.servers, ...m.labels, ...m.arrows, m.listen, m.stop, m.home, m.houseOut];
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(m.arrows).toHaveLength(3);
    expect(m.servers).toHaveLength(2);
  });

  it("lifts the house off before the logo is drawn, then writes the command and the link", () => {
    expect(m.houseOut + d.houseOut).toBeLessThanOrEqual(m.logo);
    const order = [m.logo, m.logoStroke, m.install, m.link, m.beatOut];
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(m.logoStroke + d.logoStroke).toBeLessThanOrEqual(m.install + 0.3);
  });

  it("tells the viewer it runs on their servers, stays home, and how to install it", () => {
    expect(beat5.captions.map((c) => c.id)).toEqual(["b5-servers", "b5-home", "b5-install"]);
    const [servers, home, install] = beat5.captions;
    expect(servers.start).toBeLessThan(m.arrows[0]);
    expect(home.start).toBeLessThanOrEqual(m.home);
    expect(home.end).toBeLessThanOrEqual(m.houseOut + d.houseOut);
    expect(install.start).toBeLessThanOrEqual(m.install);
    for (const lang of LANGS) expect(install.text[lang]).toContain("Homebrew");
  });

  it("leaves the last second blank paper: the Beat and its Captions are gone before the closing fade", () => {
    expect(m.beatOut + d.beatOut).toBeLessThanOrEqual(LAST_SECOND);
    for (const c of beat5.captions) expect(c.end, c.id).toBeLessThanOrEqual(LAST_SECOND);
    expect(m.link + 0.6 + 0.5).toBeLessThanOrEqual(m.beatOut);
  });
});

describe("Beat 5 sound", () => {
  it("chimes the Dictation's start and stop on the Mac", () => {
    expect(cuesOf("rec_start")).toEqual([m.listen]);
    expect(cuesOf("rec_stop")).toEqual([m.stop]);
  });

  it("scratches the pen for the house, the labels, each arrow, the note, the W, the command and the link", () => {
    expect(cuesOf("pen_scratch")).toEqual([
      m.sketchIn,
      ...m.labels,
      ...m.arrows,
      m.home,
      m.logoStroke,
      m.install,
      m.link,
    ]);
  });
});

describe("Beat 5 text", () => {
  const charset = new Set(fs.readFileSync(path.join(ROOT, "public/fonts/LXGWWenKai-Regular.subset.charset.txt"), "utf8"));
  const missing = (s: string) => [...s].filter((ch) => ch !== " " && !charset.has(ch));

  it("handwrites its notes, server names and the link in the bundled font subset", () => {
    for (const lang of LANGS) {
      const { settings } = COPY[lang].whisper;
      for (const s of [BEAT5_TEXT.mac[lang], BEAT5_TEXT.home[lang], settings.speechToText, settings.cleanup, COPY[lang].outro.link]) {
        expect(missing(s), `${lang}: ${s}`).toEqual([]);
      }
    }
  });

  it("labels the servers with local endpoints and shows the Homebrew install command", () => {
    for (const lang of LANGS) {
      const { settings } = COPY[lang].whisper;
      for (const url of [settings.asrUrl, settings.cleanupUrl]) {
        expect(url).toMatch(/^http:\/\/(localhost|127\.0\.0\.1|192\.168\.\d+\.\d+|10\.\d+\.\d+\.\d+)(:\d+)?\//);
      }
      expect(COPY[lang].outro.install).toMatch(/^brew install /);
      expect(COPY[lang].outro.link).toBe("github.com/softmaxe/whisper");
    }
  });
});

describe("Review frames", () => {
  const at = Object.fromEntries((beat5.reviewFrames ?? []).map((f) => [f.name, f.at]));

  it("catch the house with data flowing, before it lifts off", () => {
    expect(at.house).toBeGreaterThan(m.home + 0.9);
    expect(at.house).toBeLessThan(m.houseOut);
  });

  it("catch the finished logo with the install command and link, before the fade", () => {
    expect(at["logo-install"]).toBeGreaterThan(m.link + 0.6);
    expect(at["logo-install"]).toBeLessThan(m.beatOut);
  });

  it("catch the blank paper of the last second", () => {
    expect(at["blank-paper"]).toBeGreaterThanOrEqual(LAST_SECOND);
    expect(at["blank-paper"]).toBeLessThan(beat5.end);
  });
});
