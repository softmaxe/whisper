import { describe, expect, it } from "vitest";
import { graphemes, revealText, spokenTokens } from "../src/lib/text.ts";

describe("revealText", () => {
  it("reveals Latin text by characters", () => {
    expect(revealText("Friday works", 0)).toBe("");
    expect(revealText("Friday works", 0.5)).toBe("Friday");
    expect(revealText("Friday works", 1)).toBe("Friday works");
  });

  it("never splits a Chinese character or emoji", () => {
    expect(revealText("周五可以。", 0.4)).toBe("周五");
    expect(revealText("👋🏽 hi", 0.34)).toBe("👋🏽");
  });

  it("clamps progress", () => {
    expect(revealText("abc", -1)).toBe("");
    expect(revealText("abc", 2)).toBe("abc");
  });
});

describe("graphemes", () => {
  it("counts user-perceived characters", () => {
    expect(graphemes("周五👋🏽").length).toBe(3);
  });
});

describe("spokenTokens", () => {
  it("splits English on spaces", () => {
    expect(spokenTokens("I'll join. Can you", "en").map((t) => t.text)).toEqual([
      "I'll",
      "join.",
      "Can",
      "you",
    ]);
  });

  it("chunks Chinese into pairs and keeps Latin words whole", () => {
    expect(spokenTokens("带上Supabase的方案", "zh-CN").map((t) => t.text)).toEqual([
      "带上",
      "Supabase",
      "的方",
      "案",
    ]);
  });
});
