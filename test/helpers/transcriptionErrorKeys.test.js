const test = require("node:test");
const assert = require("node:assert/strict");

const en = require("../../src/locales/en/translation.json");

const load = () => import("../../src/components/notes/shared.ts");

// Codes transcriptionRoute surfaces to the upload UI. Anything added there
// needs a locale key or it reaches the user as a raw English main-process string.
const MAPPED_CODES = ["CUSTOM_ENDPOINT_INVALID"];

test("every mapped code resolves to a key that exists in en", async () => {
  const { transcriptionErrorKey } = await load();

  for (const code of MAPPED_CODES) {
    const key = transcriptionErrorKey({ code });
    assert.ok(key, `${code} has no i18n key`);
    assert.ok(en.notes.upload[key], `notes.upload.${key} is missing from en`);
  }
});

test("unmapped, codeless and absent failures fall through to the raw message", async () => {
  const { transcriptionErrorKey } = await load();

  assert.equal(transcriptionErrorKey({ code: "SERVER_ERROR" }), undefined);
  assert.equal(transcriptionErrorKey(new Error("boom")), undefined);
  assert.equal(transcriptionErrorKey(null), undefined);
  assert.equal(transcriptionErrorKey(undefined), undefined);
});
