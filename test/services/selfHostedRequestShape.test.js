const test = require("node:test");
const assert = require("node:assert/strict");

const load = () => import("../../src/services/ai/chatRequestBody.ts");
const MAX_TOKENS = 1000;

const CASES = [
  [
    "Ollama thinking suppression",
    "qwen3.5:9b",
    { disableThinking: true },
    {
      max_tokens: MAX_TOKENS,
      temperature: 0,
      reasoning: { effort: "none" },
      chat_template_kwargs: { enable_thinking: false },
    },
  ],
  [
    "gpt-oss minimum reasoning effort",
    "gpt-oss-20b-mxfp4",
    { disableThinking: true },
    {
      max_tokens: MAX_TOKENS,
      temperature: 0,
      reasoning_effort: "low",
      reasoning: { effort: "low" },
      chat_template_kwargs: { enable_thinking: false },
    },
  ],
  [
    "custom cleanup prompts retain the gpt-oss effort floor",
    "gpt-oss-20b-mxfp4",
    { systemPrompt: "Clean the transcript.", requireCompleteOutput: true, temperature: 0 },
    { max_tokens: MAX_TOKENS, temperature: 0, reasoning_effort: "low" },
  ],
];

for (const [name, model, config, expectedParams] of CASES) {
  test(`self-hosted request: ${name}`, async () => {
    const { applyChatCompletionsParams } = await load();
    const body = { model, messages: [] };
    applyChatCompletionsParams(body, { model, provider: "lan", config, maxTokens: MAX_TOKENS });
    const { model: _model, messages: _messages, ...params } = body;
    assert.deepEqual(params, expectedParams);
  });
}

test("self-hosted requests preserve explicit temperature and token limits", async () => {
  const { applyChatCompletionsParams } = await load();
  const body = { model: "qwen3.5:9b", messages: [] };
  applyChatCompletionsParams(body, {
    model: body.model,
    provider: "lan",
    config: { temperature: 0.7, maxTokens: 42 },
    maxTokens: 42,
  });
  assert.equal(body.temperature, 0.7);
  assert.equal(body.max_tokens, 42);
});
