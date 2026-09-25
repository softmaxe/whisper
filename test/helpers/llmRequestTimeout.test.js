const test = require("node:test");
const assert = require("node:assert/strict");

const load = () => import("../../src/helpers/llmRequestTimeout.js");

test("LLM requests use the fixed 30-second timeout", async () => {
  const { getLlmRequestTimeoutSeconds } = await load();

  assert.equal(typeof getLlmRequestTimeoutSeconds, "function");
  assert.equal(getLlmRequestTimeoutSeconds(), 30);
});

test("a deadline error carries the timeout code and the seconds it waited", async () => {
  const { llmRequestTimeoutError, LLM_REQUEST_TIMEOUT_CODE } = await load();

  const error = llmRequestTimeoutError(30);
  assert.ok(error instanceof Error);
  assert.equal(error.message, "Request timed out after 30s");
  assert.equal(error.code, LLM_REQUEST_TIMEOUT_CODE);
});
