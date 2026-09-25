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

/**
 * A sentence as the words a listener hears arrive: English by words, Chinese
 * in two-character beats with embedded Latin words kept whole.
 */
export function spokenTokens(text: string, lang: "en" | "zh-CN"): { text: string }[] {
  if (lang === "en") return text.split(" ").map((word) => ({ text: word }));
  const tokens: { text: string }[] = [];
  for (const part of text.match(/[A-Za-z0-9.:/-]+|[^A-Za-z0-9.:/-]+/g) ?? []) {
    if (/^[A-Za-z0-9.:/-]+$/.test(part)) {
      tokens.push({ text: part });
      continue;
    }
    const characters = graphemes(part);
    for (let i = 0; i < characters.length; i += 2) {
      tokens.push({ text: characters.slice(i, i + 2).join("") });
    }
  }
  return tokens;
}
