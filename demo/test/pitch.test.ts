import { describe, expect, it } from "vitest";
import { nearestSample, noteNameToMidi } from "../src/music/pitch.ts";

describe("noteNameToMidi", () => {
  it("reads sharps written with # or s", () => {
    expect(noteNameToMidi("C4")).toBe(60);
    expect(noteNameToMidi("A#2")).toBe(46);
    expect(noteNameToMidi("Ds1")).toBe(27);
    expect(noteNameToMidi("Fs7")).toBe(102);
    expect(noteNameToMidi("A0")).toBe(21);
  });

  it("rejects anything else", () => {
    expect(noteNameToMidi("H2")).toBeNull();
    expect(noteNameToMidi("C")).toBeNull();
  });
});

describe("nearestSample", () => {
  const samples = [{ midi: 48 }, { midi: 51 }, { midi: 54 }];

  it("picks the closest root and prefers pitching down on a tie", () => {
    expect(nearestSample(samples, 49)).toEqual({ midi: 48 });
    expect(nearestSample(samples, 50)).toEqual({ midi: 51 });
    expect(nearestSample(samples, 60)).toEqual({ midi: 54 });
  });
});
