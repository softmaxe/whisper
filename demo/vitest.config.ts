import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    testTimeout: 30_000,
    projects: [
      {
        // Timeline rules, caption layout and helpers: fast, no build needed.
        extends: true,
        test: { name: "unit", include: ["test/*.test.ts", "test/timeline/**/*.test.ts", "test/video/**/*.test.ts"] },
      },
      {
        // The rendered Film cuts; the global setup rebuilds them first when missing or stale.
        extends: true,
        test: { name: "film", include: ["test/film/**/*.test.ts"], globalSetup: ["test/film/global-setup.ts"] },
      },
    ],
  },
});
