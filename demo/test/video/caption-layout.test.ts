/**
 * Caption line breaking: a wrapped Caption breaks at clause punctuation or
 * between words, never inside a phrase or a word.
 */
import { describe, expect, it } from "vitest";
import { clauses, layoutLines, tokenize } from "../../src/film/components/captionLayout.ts";

describe("tokenize", () => {
  it("keeps Latin words whole, including apostrophes and links", () => {
    expect(tokenize("it's cal.com/alex/30min")).toEqual(["it's", " ", "cal.com/alex/30min"]);
  });

  it("glues punctuation to its neighbour", () => {
    expect(tokenize("打字太慢？")).toEqual(["打", "字", "太", "慢？"]);
    expect(tokenize("Slow, right?")).toEqual(["Slow,", " ", "right?"]);
  });

  it("keeps a short quoted phrase and a reduplicated pair whole", () => {
    expect(tokenize("说“你好”看看")).toEqual(["说", "“你好”", "看看"]);
  });
});

describe("clauses", () => {
  it("lets a clause too long for one line wrap inside", () => {
    const tokens = tokenize("一二三四五六七八九十");
    expect(clauses(tokens, 100, 500)).toEqual([{ tokens: tokens.map((_, i) => i), keep: false }]);
  });
});

describe("layoutLines", () => {
  it("keeps a short Caption on one line", () => {
    expect(layoutLines("打字太慢？", 64, 1560)).toEqual(["打字太慢？"]);
  });

  it("breaks between clauses when the whole Caption does not fit", () => {
    expect(layoutLines("口头禅删掉，标点补上。", 100, 700)).toEqual(["口头禅删掉，", "标点补上。"]);
  });

  it("wraps English between words", () => {
    const lines = layoutLines("one two three four five six", 100, 700);
    expect(lines.length).toBeGreaterThan(1);
    for (const line of lines) expect(line).toBe(line.trim());
    expect(lines.join(" ")).toBe("one two three four five six");
  });
});
