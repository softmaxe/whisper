const test = require("node:test");
const assert = require("node:assert/strict");

const { createRendererServer, installBrowserGlobals } = require("../lib/rendererTestHarness");

const OVERRIDDEN_VOICE_SURFACE_GEOMETRY = `
  export const ASSISTANT_PANEL_SIZE_LIMITS = {
    ratioWidth: 500,
    ratioHeight: 625,
    gutter: 31,
    minSurfaceWidth: 300,
    minSurfaceHeight: 120,
    maxSurfaceWidth: 700,
  };
  export const LIVE_TRANSCRIPT_SURFACE_LIMITS = { minHeight: 91, maxHeight: 321 };
`;

test("renderer presentation consumes the shared live-transcript geometry", async (t) => {
  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-voice-surface-geometry-test-",
    mockModules: {
      "/voiceSurfaceGeometry.mjs": OVERRIDDEN_VOICE_SURFACE_GEOMETRY,
    },
  });

  const presentation = await vite.ssrLoadModule("/helpers/voicePillPresentation.js");

  assert.deepEqual(presentation.LIVE_TRANSCRIPT_SURFACE_LIMITS, {
    minHeight: 91,
    maxHeight: 321,
  });
});

test("DictationErrorCard reports content height at the shared expected width", async (t) => {
  const card = {
    getBoundingClientRect: () => ({ width: 500 }),
    offsetHeight: 120,
    scrollHeight: 137,
  };
  const reportedHeights = [];
  const originalResizeObserver = globalThis.ResizeObserver;
  const originalRequestAnimationFrame = globalThis.requestAnimationFrame;
  const originalCancelAnimationFrame = globalThis.cancelAnimationFrame;
  installBrowserGlobals(t, {
    window: { screen: { availWidth: 1000, availHeight: 1000 } },
  });
  globalThis.__dictationErrorCard = card;
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
  };
  globalThis.requestAnimationFrame = (callback) => {
    callback(0);
    return 1;
  };
  globalThis.cancelAnimationFrame = () => {};
  t.after(() => {
    globalThis.__dictationErrorCardCleanup?.();
    delete globalThis.__dictationErrorCard;
    delete globalThis.__dictationErrorCardCleanup;
    if (originalResizeObserver === undefined) delete globalThis.ResizeObserver;
    else globalThis.ResizeObserver = originalResizeObserver;
    if (originalRequestAnimationFrame === undefined) delete globalThis.requestAnimationFrame;
    else globalThis.requestAnimationFrame = originalRequestAnimationFrame;
    if (originalCancelAnimationFrame === undefined) delete globalThis.cancelAnimationFrame;
    else globalThis.cancelAnimationFrame = originalCancelAnimationFrame;
  });

  const vite = await createRendererServer(t, {
    cachePrefix: "openwhispr-dictation-error-geometry-test-",
    noExternal: true,
    mockModules: {
      "react/jsx-dev-runtime": `
        export const Fragment = Symbol.for("react.fragment");
        export function jsxDEV(type, props, key) { return { type, props, key }; }
      `,
      "/components/icons": `
        export function RotateCcw() { return null; }
        export function ScrollText() { return null; }
      `,
      react: `
        export function useLayoutEffect(effect) {
          globalThis.__dictationErrorCardCleanup = effect();
        }
        export function useRef(initialValue) {
          return { current: initialValue === null ? globalThis.__dictationErrorCard : initialValue };
        }
      `,
      "/voiceSurfaceGeometry.mjs": OVERRIDDEN_VOICE_SURFACE_GEOMETRY,
    },
  });
  const { DictationErrorCard } = await vite.ssrLoadModule(
    "/components/dictation/DictationErrorCard.tsx"
  );

  DictationErrorCard({
    actions: [],
    onAction() {},
    onPreferredHeightChange: (height) => reportedHeights.push(height),
  });

  assert.deepEqual(reportedHeights, [137]);
});
