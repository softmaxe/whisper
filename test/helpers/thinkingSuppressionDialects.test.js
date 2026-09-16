const test = require("node:test");
const assert = require("node:assert/strict");

// Requires Node's native TypeScript type-stripping (Node >= 22.6 with
// --experimental-strip-types, on by default in Node 23.6+/24). CI runs Node 24.

const load = () => import("../../src/services/ai/thinkingSuppressionDialects.ts");

test("lan gets the nested reasoning object plus chat_template_kwargs", async () => {
  const { suppressThinking } = await load();

  const body = {};
  suppressThinking(body, "lan", "qwen3-8b");

  assert.deepEqual(body, {
    reasoning: { effort: "none" },
    chat_template_kwargs: { enable_thinking: false },
  });
});

test("lan sends a family's suppress floor inside the reasoning object, not a flat effort", async () => {
  const { suppressThinking } = await load();

  const body = {};
  suppressThinking(body, "lan", "gpt-oss-20b-mxfp4");

  assert.deepEqual(body, {
    reasoning: { effort: "low" },
    chat_template_kwargs: { enable_thinking: false },
  });
  assert.ok(!("reasoning_effort" in body), "flat reasoning_effort trips vLLM on lan (#1611)");
});

test("the gpt-oss floor covers the whole family case-insensitively", async () => {
  const { suppressThinking } = await load();

  const safeguard = {};
  suppressThinking(safeguard, "lan", "gpt-oss-safeguard-120b");
  assert.equal(safeguard.reasoning.effort, "low");

  const mixedCase = {};
  suppressThinking(mixedCase, "lan", "GPT-OSS-20B");
  assert.equal(mixedCase.reasoning.effort, "low");
});
