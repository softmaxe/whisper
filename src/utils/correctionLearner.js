/**
 * Extracts transcription corrections by diffing original text against
 * the edited field value. Returns corrected terms to add to the custom dictionary.
 */

// Swapping one everyday word for another ("why" -> "what") is a content edit,
// not vocabulary; learning it would only pad the cleanup prompt. Words under
// three letters are already dropped, so none are listed here.
const COMMON_WORDS = new Set(
  `the and for not with you this but his from they say her she will one all would
  there their what out about who get which when make can like time just him know
  take into year your good some could them see other than then now look only come
  over think also back after use two how our work first well way even new want
  because any these give day most are was were been has had did does said went
  made got came took saw knew thought where why here very much many still too again
  off down never every own same another both each few more less last next while
  before through under between should might must being have that its yes okay
  这个 那个 这些 那些 这样 那样 我们 你们 他们 她们 什么 怎么 为什么 因为 所以 但是
  可以 已经 现在 然后 就是 还是 如果 没有 一个 一下 知道 觉得 感觉 需要 应该 时候`.split(/\s+/)
);

// A dictionary entry is a name or term, not a clause: longer replacements are
// rewrites even when the rest of the sentence survives.
const MAX_TERM_UNITS = 4;
const MAX_REPLACED_UNITS = 8;

// Scripts written without spaces between words. Spelling distance means
// nothing for their mishearings ("克劳德" -> "Claude"), so they skip that check.
const UNSPACED_SCRIPT = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u;
const UNSPACED_SCRIPT_ONLY = /^[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}ー\s]+$/u;

// Word segmentation that also splits unspaced scripts (ICU's dictionary
// breaker); whitespace splitting would treat a whole Chinese sentence as one word.
const wordSegmenter = new Intl.Segmenter(undefined, { granularity: "word" });

/** Levenshtein edit distance between two strings */
function editDistance(a, b) {
  const m = a.length;
  const n = b.length;
  const dp = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));

  for (let i = 0; i <= m; i++) dp[i][0] = i;
  for (let j = 0; j <= n; j++) dp[0][j] = j;

  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (a[i - 1] === b[j - 1]) {
        dp[i][j] = dp[i - 1][j - 1];
      } else {
        dp[i][j] = 1 + Math.min(dp[i - 1][j], dp[i][j - 1], dp[i - 1][j - 1]);
      }
    }
  }
  return dp[m][n];
}

/**
 * Tokenize text into word units with their source offsets, so a changed run
 * can be sliced back out with its original spacing ("Claude Code").
 */
function tokenize(text) {
  const units = [];
  for (const { segment, index, isWordLike } of wordSegmenter.segment(text)) {
    if (!isWordLike) continue;
    units.push({
      text: segment,
      key: segment.toLowerCase(),
      start: index,
      end: index + segment.length,
    });
  }
  return units;
}

/**
 * Find the region in fieldValue that corresponds to the pasted originalText.
 * If the field only contains the pasted text, returns fieldValue as-is.
 */
function findEditedRegion(originalText, fieldValue) {
  if (fieldValue.length <= originalText.length * 1.5) {
    return fieldValue;
  }

  const idx = fieldValue.indexOf(originalText);
  if (idx !== -1) {
    return originalText;
  }

  // Sliding window: find the region with highest word overlap
  const origUnits = tokenize(originalText);
  const fieldUnits = tokenize(fieldValue);
  const windowSize = origUnits.length;

  if (fieldUnits.length <= windowSize) {
    return fieldValue;
  }

  let bestStart = 0;
  let bestScore = -1;

  for (let i = 0; i <= fieldUnits.length - windowSize; i++) {
    let matches = 0;
    for (let j = 0; j < windowSize; j++) {
      if (fieldUnits[i + j].key === origUnits[j].key) {
        matches++;
      }
    }
    if (matches > bestScore) {
      bestScore = matches;
      bestStart = i;
    }
  }

  // Require at least 30% word overlap to consider it a match
  if (bestScore < windowSize * 0.3) {
    return fieldValue;
  }

  return fieldValue.slice(fieldUnits[bestStart].start, fieldUnits[bestStart + windowSize - 1].end);
}

/**
 * Word-level LCS diff. Returns how many units (and their characters) were kept,
 * and the changed runs ("hunks"): consecutive removed original units and added
 * edited units.
 */
function diffUnits(origUnits, editedUnits) {
  const m = origUnits.length;
  const n = editedUnits.length;

  const dp = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));
  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (origUnits[i - 1].key === editedUnits[j - 1].key) {
        dp[i][j] = dp[i - 1][j - 1] + 1;
      } else {
        dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
      }
    }
  }

  const hunks = [];
  let keptChars = 0;
  let current = null;
  const flush = () => {
    if (current) hunks.unshift(current);
    current = null;
  };

  let i = m,
    j = n;
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && origUnits[i - 1].key === editedUnits[j - 1].key) {
      flush();
      keptChars += origUnits[i - 1].text.length;
      i--;
      j--;
      continue;
    }
    current = current || { removed: [], added: [] };
    if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      current.added.unshift(editedUnits[j - 1]);
      j--;
    } else {
      current.removed.unshift(origUnits[i - 1]);
      i--;
    }
  }
  flush();

  return { keptUnits: dp[m][n], keptChars, hunks };
}

function sliceUnits(text, units) {
  return text.slice(units[0].start, units[units.length - 1].end);
}

/** Whether a replaced run looks like a misheard term rather than a content edit. */
function isLearnableTerm(replaced, term, termUnits) {
  if (!/\p{L}/u.test(term)) return false;
  if (termUnits.every((unit) => COMMON_WORDS.has(unit.key))) return false;

  if (UNSPACED_SCRIPT_ONLY.test(term)) {
    // The segmenter's dictionary already knows a single-unit term (认为, 说得),
    // so it is ordinary vocabulary or a grammar fix. Misheard names and jargon
    // fall outside that dictionary and split into several units (章|珊).
    return termUnits.length >= 2;
  }

  if (UNSPACED_SCRIPT.test(replaced)) {
    // A Latin term replacing an unspaced-script mishearing ("爱爱" -> "AI").
    return term.length >= 2;
  }

  if (term.length < 3) return false;

  // 0.65 threshold allows phonetic corrections like "Shunade" → "Sinead" (dist 4/7 = 0.57)
  // while filtering out unrelated word replacements.
  const a = replaced.toLowerCase();
  const b = term.toLowerCase();
  return editDistance(a, b) / Math.max(a.length, b.length) <= 0.65;
}

/**
 * Extract corrected terms from a user's edits to pasted transcription text.
 *
 * @param {string} originalText - The text that was originally pasted (from transcription)
 * @param {string} fieldValue - The current value of the text field (after user edits)
 * @param {string[]} existingDictionary - Words already in the custom dictionary
 * @returns {string[]} Array of corrected terms to add to the dictionary
 */
function extractCorrections(originalText, fieldValue, existingDictionary) {
  if (!originalText || !fieldValue) return [];
  if (originalText === fieldValue) return [];

  const editedRegion = findEditedRegion(originalText, fieldValue);
  if (editedRegion === originalText) return [];

  const origUnits = tokenize(originalText);
  const editedUnits = tokenize(editedRegion);

  if (origUnits.length === 0 || editedUnits.length === 0) return [];

  // If most of the text changed, this is a rewrite, not corrections. Counting
  // characters as well as words keeps a misheard name in an unspaced script,
  // which splits into one unit per character, from outweighing a short sentence.
  const { keptUnits, keptChars, hunks } = diffUnits(origUnits, editedUnits);
  const totalChars = origUnits.reduce((sum, unit) => sum + unit.text.length, 0);
  if (keptUnits * 2 < origUnits.length && keptChars * 3 < totalChars) return [];

  const safeDict = Array.isArray(existingDictionary) ? existingDictionary : [];
  const dictSet = new Set(safeDict.map((w) => w.toLowerCase()));
  const seenCorrections = new Set();
  const results = [];

  for (const { removed, added } of hunks) {
    // Pure insertions and deletions add or drop content; only a replacement
    // pairs a misheard run with its correction.
    if (removed.length === 0 || added.length === 0) continue;
    if (added.length > MAX_TERM_UNITS || removed.length > MAX_REPLACED_UNITS) continue;

    const replaced = sliceUnits(originalText, removed);
    const term = sliceUnits(editedRegion, added);
    const normalizedTerm = term.toLowerCase();

    if (dictSet.has(normalizedTerm)) continue;
    if (seenCorrections.has(normalizedTerm)) continue;
    if (replaced.toLowerCase() === normalizedTerm) continue;
    if (!isLearnableTerm(replaced, term, added)) continue;

    results.push(term);
    seenCorrections.add(normalizedTerm);
  }

  return results;
}

module.exports = { extractCorrections };
