const test = require("node:test");
const assert = require("node:assert/strict");
const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

async function loadCleanup(t) {
  installBrowserGlobals(t);
  const vite = await createRendererServer(t, { cachePrefix: "whisper-self-hosted-cleanup-" });
  const service = (await vite.ssrLoadModule("/services/ReasoningService.ts")).default;
  const { useSettingsStore } = await vite.ssrLoadModule("/stores/settingsStore.ts");
  const { default: i18n } = await vite.ssrLoadModule("/i18n.ts");
  await i18n.changeLanguage("en");
  t.after(() => service.destroy());
  const originalFetch = globalThis.fetch;
  t.after(() => {
    globalThis.fetch = originalFetch;
  });
  useSettingsStore.setState({
    cleanupMode: "self-hosted",
    cleanupRemoteUrl: "http://127.0.0.1:8080",
    cleanupModel: "qwen3.5:9b",
    cleanupCustomApiKey: "fixture-key",
  });
  return { service, settings: useSettingsStore };
}

test("self-hosted cleanup uses the saved endpoint, model, key, and prompt", async (t) => {
  const { service } = await loadCleanup(t);
  const requests = [];
  globalThis.fetch = async (url, init) => {
    requests.push({ url, headers: init.headers, body: JSON.parse(init.body) });
    return new Response(
      JSON.stringify({
        choices: [{ message: { content: "Cleaned transcript" }, finish_reason: "stop" }],
      }),
      { headers: { "Content-Type": "application/json" } }
    );
  };

  const result = await service.processText("raw transcript", "qwen3.5:9b", null, {
    systemPrompt: "Clean the transcript.",
    requireCompleteOutput: true,
    disableThinking: true,
    temperature: 0,
  });

  assert.equal(result, "Cleaned transcript");
  assert.equal(requests.length, 1);
  assert.equal(requests[0].url, "http://127.0.0.1:8080/v1/chat/completions");
  assert.equal(requests[0].headers.Authorization, "Bearer fixture-key");
  assert.equal(requests[0].body.model, "qwen3.5:9b");
  assert.equal(requests[0].body.messages[0].content, "Clean the transcript.");
  assert.equal(requests[0].body.temperature, 0);
  assert.deepEqual(requests[0].body.chat_template_kwargs, { enable_thinking: false });
});

test("self-hosted cleanup rejects missing or unsafe endpoints before dispatch", async (t) => {
  const { service, settings } = await loadCleanup(t);
  globalThis.fetch = () => assert.fail("Invalid endpoints must not dispatch a request");

  for (const cleanupRemoteUrl of ["", "http://public.example.com/v1", "ftp://192.168.1.20/v1"]) {
    settings.setState({ cleanupRemoteUrl });
    await assert.rejects(service.processText("raw transcript", "qwen3.5:9b"), {
      message: require("../../src/locales/en/translation.json").reasoning.custom.httpsRequired,
    });
  }
});

test("self-hosted cleanup rejects incomplete and empty output", async (t) => {
  const { service } = await loadCleanup(t);
  for (const [content, finishReason, messageKey] of [
    ["Partial", "length", "hooks.audioRecording.errorDescriptions.cleanupTruncated"],
    ["", "stop", "hooks.audioRecording.errorDescriptions.cleanupEmptyReply"],
  ]) {
    globalThis.fetch = async () =>
      new Response(
        JSON.stringify({ choices: [{ message: { content }, finish_reason: finishReason }] }),
        { headers: { "Content-Type": "application/json" } }
      );
    await assert.rejects(
      service.processText("raw transcript", "qwen3.5:9b", null, { requireCompleteOutput: true }),
      { messageKey }
    );
  }
});

test("self-hosted cleanup stops after 30 seconds without retrying the timeout", async (t) => {
  const { service } = await loadCleanup(t);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let requests = 0;
  globalThis.fetch = (_url, init) => {
    requests += 1;
    return new Promise((_resolve, reject) => {
      init.signal.addEventListener("abort", () => {
        reject(Object.assign(new Error("The operation was aborted"), { name: "AbortError" }));
      });
    });
  };
  const request = assert.rejects(service.processText("raw transcript", "qwen3.5:9b"), {
    message: "Request timed out after 30s",
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(requests, 1);
  t.mock.timers.tick(30_000);
  await request;
  assert.equal(requests, 1);
});
