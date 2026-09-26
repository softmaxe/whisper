const test = require("node:test");
const assert = require("node:assert/strict");

test("cleanup failures accumulate and each batch is consumed only once", async () => {
  const store = await import("../../src/stores/cleanupFailureStore.ts");
  store.useCleanupFailureStore.setState({ pending: 0 });

  assert.equal(store.consumeCleanupFailures(), 0);
  store.recordCleanupFailure();
  store.recordCleanupFailure();
  assert.equal(store.consumeCleanupFailures(), 2);
  assert.equal(store.consumeCleanupFailures(), 0);

  store.recordCleanupFailure();
  assert.equal(store.consumeCleanupFailures(), 1);
  assert.equal(store.consumeCleanupFailures(), 0);
});
