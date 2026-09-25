const assert = require("node:assert/strict");
const test = require("node:test");

const load = () => import("../../src/utils/i18nDocument.ts");

function createI18n(language) {
  const listeners = new Set();
  return {
    language,
    resolvedLanguage: language,
    on(event, listener) {
      assert.equal(event, "languageChanged");
      listeners.add(listener);
    },
    off(event, listener) {
      assert.equal(event, "languageChanged");
      listeners.delete(listener);
    },
    emit(nextLanguage) {
      this.language = nextLanguage;
      this.resolvedLanguage = nextLanguage;
      for (const listener of listeners) listener(nextLanguage);
    },
    listenerCount: () => listeners.size,
  };
}

test("document language binding initializes and updates lang", async () => {
  const { bindDocumentLanguage } = await load();
  const i18n = createI18n("zh-CN");
  const root = { lang: "" };

  const dispose = bindDocumentLanguage(i18n, root);
  assert.deepEqual(root, { lang: "zh-CN" });

  i18n.emit("en");
  assert.deepEqual(root, { lang: "en" });

  dispose();
  assert.equal(i18n.listenerCount(), 0);
});

test("HMR disposal removes the exact language listener and remains idempotent", async () => {
  const { bindDocumentLanguage } = await load();
  const i18n = createI18n("en");
  const root = { lang: "" };
  let hotDispose;
  const hot = {
    dispose(callback) {
      hotDispose = callback;
    },
  };

  const dispose = bindDocumentLanguage(i18n, root, hot);
  assert.equal(i18n.listenerCount(), 1);
  assert.equal(typeof hotDispose, "function");

  hotDispose();
  dispose();
  assert.equal(i18n.listenerCount(), 0);
  i18n.emit("zh-CN");
  assert.deepEqual(root, { lang: "en" });
});
