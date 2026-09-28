const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });

/** User-perceived characters, so CJK text and emoji reveal whole. */
export const graphemes = (text: string) =>
  Array.from(segmenter.segment(text), (segment) => segment.segment);

/** The leading share of `text` shown at `progress` (0..1) of a typing reveal. */
export function revealText(text: string, progress: number) {
  const characters = graphemes(text);
  const count = Math.round(Math.min(1, Math.max(0, progress)) * characters.length);
  return characters.slice(0, count).join("");
}
