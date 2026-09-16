const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { createRoot } = require("react-dom/client");
const { renderToStaticMarkup } = require("react-dom/server");
const {
  createRendererServer,
  installBrowserGlobals,
  installHookDom,
} = require("../lib/rendererTestHarness");

test("cleanup toast localizes server recovery guidance and keeps fallback status quieter", async (t) => {
  let root = null;
  t.after(async () => {
    if (root) await React.act(async () => root.unmount());
    delete globalThis.__cleanupFailureToasts;
  });
  globalThis.__cleanupFailureToasts = [];
  installBrowserGlobals(t);
  const container = installHookDom(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-cleanup-failure-toast-",
    mockModules: {
      "/ui/useToast": `
        export const useToast = () => ({
          toast: (props) => globalThis.__cleanupFailureToasts.push(props)
        });
      `,
      "/utils/windowContext": `
        export const isDictationPanelWindow = () => false;
      `,
    },
  });
  const { default: CleanupFailureToastListener } = await vite.ssrLoadModule(
    "/components/CleanupFailureToastListener.tsx"
  );
  const { default: i18n } = await vite.ssrLoadModule("/i18n.ts");
  await i18n.changeLanguage("zh-CN");
  const { recordCleanupFailure, useCleanupFailureStore } = await vite.ssrLoadModule(
    "/stores/cleanupFailureStore.ts"
  );
  useCleanupFailureStore.setState({ pending: 0, lastMessage: "", lastFailure: null });

  root = createRoot(container);
  await React.act(async () => root.render(React.createElement(CleanupFailureToastListener)));

  const failure = {
    message:
      "Endpoint rejected the request (401/403). Add an API key or adjust server auth settings.",
    messageKey: "reasoning.custom.endpointUnauthorized",
    action: "Optional. Sent as a Bearer token to your text cleanup server.",
    actionKey: "reasoning.custom.apiKeyHelp",
    copyCommand: "curl -I https://server.example/v1/models",
    technicalDetails: {
      status: 401,
      underlyingError: "Unauthorized",
    },
  };
  await React.act(async () => recordCleanupFailure(failure));

  assert.equal(globalThis.__cleanupFailureToasts.length, 1);
  assert.deepEqual(globalThis.__cleanupFailureToasts[0], {
    title: "端点拒绝了请求（401/403）。请添加 API Key 或调整服务器认证设置。",
    description: "可选，将作为 Bearer token 发送给文本整理服务器。",
    secondaryDescription: "你的听写已直接粘贴，未经 AI 清理。",
    copyCommand: "curl -I https://server.example/v1/models",
    technicalDetails: failure.technicalDetails,
    variant: "destructive",
    duration: 10_000,
  });
});

test("technical AWS details use the selected UI language", async (t) => {
  installBrowserGlobals(t);
  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-technical-error-details-",
  });
  const { TechnicalErrorDetails } = await vite.ssrLoadModule(
    "/components/ui/TechnicalErrorDetails.tsx"
  );
  const { default: i18n } = await vite.ssrLoadModule("/i18n.ts");
  await i18n.changeLanguage("zh-CN");

  const markup = renderToStaticMarkup(
    React.createElement(TechnicalErrorDetails, {
      details: {
        status: 503,
        exceptionType: "ServiceUnavailableException",
        requestId: "request-123",
        underlyingError: "Bedrock overloaded",
      },
    })
  );

  assert.match(markup, /技术详情/);
  assert.match(markup, /HTTP 状态: 503/);
  assert.match(markup, /AWS 异常: ServiceUnavailableException/);
  assert.match(markup, /AWS 请求 ID: request-123/);
  assert.match(markup, /底层错误: Bedrock overloaded/);
  assert.match(markup, /aria-label="复制技术详情"/);
});
