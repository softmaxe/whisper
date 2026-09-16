const { describe, it } = require("node:test");
const assert = require("node:assert/strict");

const { normalizeUiLanguage } = require("../../src/helpers/i18nMain");

describe("normalizeUiLanguage", () => {
  it("keeps English and Simplified Chinese tags, including region variants", () => {
    assert.equal(normalizeUiLanguage("zh-CN"), "zh-CN");
    assert.equal(normalizeUiLanguage("zh_CN"), "zh-CN");
    assert.equal(normalizeUiLanguage("en"), "en");
    assert.equal(normalizeUiLanguage("en-US"), "en");
  });

  it("maps all Chinese script and region tags onto Simplified Chinese", () => {
    assert.equal(normalizeUiLanguage("zh-Hans"), "zh-CN");
    assert.equal(normalizeUiLanguage("zh-Hans-CN"), "zh-CN");
    assert.equal(normalizeUiLanguage("zh"), "zh-CN");
    assert.equal(normalizeUiLanguage("zh_TW"), "zh-CN");
    assert.equal(normalizeUiLanguage("zh-Hant"), "zh-CN");
    assert.equal(normalizeUiLanguage("zh-Hant-TW"), "zh-CN");
    assert.equal(normalizeUiLanguage("zh_Hant_TW"), "zh-CN");
    assert.equal(normalizeUiLanguage("zh-HK"), "zh-CN");
  });

  it("falls back to English for removed and unknown languages", () => {
    assert.equal(normalizeUiLanguage("pt-BR"), "en");
    assert.equal(normalizeUiLanguage("de-DE"), "en");
    assert.equal(normalizeUiLanguage("ar"), "en");
    assert.equal(normalizeUiLanguage("ko-KR"), "en");
    assert.equal(normalizeUiLanguage(""), "en");
  });
});
