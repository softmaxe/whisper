const test = require("node:test");
const assert = require("node:assert/strict");

const { extractCorrections } = require("../../src/utils/correctionLearner.js");

test("null or empty inputs yield no corrections", () => {
  assert.deepEqual(extractCorrections(null, "hello", []), []);
  assert.deepEqual(extractCorrections("hello", null, []), []);
  assert.deepEqual(extractCorrections("", "hello", []), []);
  assert.deepEqual(extractCorrections("hello", "", []), []);
});

test("identical texts yield no corrections", () => {
  assert.deepEqual(extractCorrections("hello world", "hello world", []), []);
});

test("a phonetic mishearing fixed by the user is learned", () => {
  // "Shunade" is a plausible transcription mishearing of "Sinead"
  const result = extractCorrections("Hey Shunade how are you", "Hey Sinead how are you", []);
  assert.ok(result.includes("Sinead"));
});

test("corrections already in the dictionary are not re-learned, case-insensitively", () => {
  const original = "Hey Shunade how are you";
  const edited = "Hey Sinead how are you";

  assert.ok(!extractCorrections(original, edited, ["Sinead"]).includes("Sinead"));
  assert.ok(!extractCorrections(original, edited, ["sinead"]).includes("Sinead"));
});

test("a wholesale rewrite is not mistaken for corrections", () => {
  const result = extractCorrections("the cat sat on the mat", "a dog stood under a rug", []);
  assert.deepEqual(result, []);
});

test("very short replacements are ignored — two-letter words are edits, not vocabulary", () => {
  const result = extractCorrections("I went to see XX today", "I went to see Al today", []);
  assert.ok(!result.includes("Al"));
});

test("unrelated word swaps are filtered by edit distance — cat to elephant is a rewrite, not a mishearing", () => {
  const result = extractCorrections("I saw a cat yesterday", "I saw a elephant yesterday", []);
  assert.ok(!result.includes("elephant"));
});

test("swapping one everyday word for another is a content edit, not vocabulary", () => {
  const original = "This is why I'm speaking";
  assert.deepEqual(extractCorrections(original, "This is what I'm speaking", []), []);
  assert.deepEqual(extractCorrections(original, "This is where I'm speaking", []), []);
});

test("a non-array dictionary is tolerated", () => {
  const result = extractCorrections("Hey Shunade", "Hey Sinead", null);
  assert.ok(result.includes("Sinead"));
});

test("the same correction appearing twice is only learned once", () => {
  const result = extractCorrections("Shunade said hi to Shunade", "Sinead said hi to Sinead", []);
  const sinead = result.filter((w) => w.toLowerCase() === "sinead");
  assert.ok(sinead.length <= 1);
});

test("a Latin term replacing a misheard Chinese transliteration is learned as a phrase", () => {
  assert.deepEqual(
    extractCorrections("我们今天用克劳德扣的写代码。", "我们今天用Claude Code写代码。", []),
    ["Claude Code"]
  );
});

test("a short Chinese sentence dominated by the misheard term is not mistaken for a rewrite", () => {
  assert.deepEqual(extractCorrections("克劳德扣的写代码", "Claude Code写代码", []), [
    "Claude Code",
  ]);
  assert.deepEqual(extractCorrections("用爱爱写代码", "用AI写代码", []), ["AI"]);
});

test("a misheard Chinese name is learned without the surrounding sentence", () => {
  assert.deepEqual(extractCorrections("明天和张三开会", "明天和章珊开会", []), ["章珊"]);
});

test("unspaced English words inside Chinese text are learned as the corrected term", () => {
  assert.deepEqual(extractCorrections("我在用open whisper做听写", "我在用OpenWhispr做听写", []), [
    "OpenWhispr",
  ]);
  assert.deepEqual(
    extractCorrections("我们用 cloud code 写代码", "我们用 Claude code 写代码", []),
    ["Claude"]
  );
});

test("Chinese grammar fixes and everyday word swaps are content edits, not vocabulary", () => {
  assert.deepEqual(extractCorrections("他说的很好", "他说得很好", []), []);
  assert.deepEqual(extractCorrections("我觉得这个方案可以", "我认为这个方案可以", []), []);
});

test("fixing a single Chinese character is not learned, as documented in the README", () => {
  // The segmenter splits an unknown name into characters, leaving only the
  // changed one; without its reading, 山 -> 珊 looks like an ordinary edit.
  assert.deepEqual(extractCorrections("我今天和张山一起吃饭", "我今天和张珊一起吃饭", []), []);
});

test("a rewritten Chinese sentence is not mistaken for corrections", () => {
  assert.deepEqual(extractCorrections("今天天气很好我们去公园", "明天下雨大家待在家里", []), []);
});

test("a correction inside a longer field is learned from the pasted region only", () => {
  const prefix = "Earlier chat content that was already in the field before dictation. ";
  assert.deepEqual(
    extractCorrections("部署到乌班图服务器上", `${prefix}部署到Ubuntu服务器上`, []),
    ["Ubuntu"]
  );
});
