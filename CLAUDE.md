# whisper maintenance

Before changing behavior, read the scope and acceptance notes in `README.md`. They define the supported features of this reduced OpenWhispr build. The retained upstream modules also contain dormant code for features outside that scope.

## Working rules

- Reuse the existing recorder, ASR requests, cleanup requests, dictionary operations, snippet expansion, and native helpers. Make a new implementation only when the retained code cannot be separated from a removed feature.
- Preserve the existing layouts, spacing, colors, icons, and interaction patterns. Ask the user before introducing a feature or making a substantial visual change. Existing components and styles can be reused directly.
- Keep inference on Self-Hosted for both transcription and cleanup. Preserve custom endpoint URLs, model names, optional credentials, and custom cleanup prompts.
- Keep the original application's data separate. The application identifier, data path, and keychain namespace belong to whisper. Test fixtures and imported personal settings stay outside version control.
- Follow `.nvmrc` for Node.js. Regenerate `package-lock.json` only with Node.js 24.

## Code map

- `main.js`, `src/helpers/windowManager.js`, and `src/helpers/hotkeyManager.js` own application startup, windows, and global shortcuts.
- `preload.js` and `src/helpers/ipcHandlers.js` define the renderer/main-process boundary. Update both when changing an IPC channel.
- `src/App.jsx` and `src/hooks/useAudioRecording.js` drive the original recording pill and dictation lifecycle.
- `src/helpers/audioManager.js` owns capture, ASR, cleanup routing, history persistence, and delivery.
- `src/services/ReasoningService.ts` and `src/config/prompts/` implement cleanup requests and prompt composition. Preserve the saved prompt and upstream language suffix behavior unless the user requests a change.
- `src/components/ControlPanel.tsx` hosts History, Insights, Upload, and Dictionary. `SettingsModal.tsx` and `SettingsPage.tsx` retain the original settings shell with the reduced set of sections.
- `src/components/InsightsView.tsx` uses the original local analytics helpers and SQLite aggregation. Exclude uploaded files from dictation metrics.
- `src/components/notes/UploadAudioView.tsx` and `src/stores/batchQueueStore.ts` reuse the original local-file upload flow. Save results to History with `routeKind: "upload"`; keep the upstream ASR-only behavior without URL imports, speaker detection, or Notes.
- `src/helpers/tray.js` owns the original menu bar icon and menu. The macOS visibility preference changes it immediately and persists across restarts.
- `src/components/DictionaryView.tsx`, `src/components/SnippetsView.tsx`, `src/utils/snippets.ts`, and `src/helpers/database.js` retain the dictionary, snippets, and SQLite implementations.
- `src/stores/settingsStore.ts` initializes self-hosted modes and keeps the removed account, assistant, translation, and fallback modes disabled.

## Typography and assets

English text uses the bundled public JetBrains Mono webfonts. Chinese text uses system PingFang on macOS. `src/brandFonts.ts` registers the bundled faces; `src/index.css` defines the fallback stacks. The font source revision and OFL license are in `src/assets/fonts/jetbrains-mono/`. Builds must remain independent of private font repositories.

Use icons from `src/components/icons/` and import existing assets instead of using root-relative URLs. Packaged renderers load from a `file://` origin. All UI copy uses the existing i18n system; any necessary new copy needs corresponding keys in every supported locale. Preserve prompt placeholders such as `{{agentName}}`.

## Verification

Use the scripts in `package.json` and the runtime guidance in `README.md`. Test the affected behaviors, then run type checking and lint. For native SQLite tests, use the same runtime ABI as the installed `better-sqlite3` binding.

For changes to the dictation flow, verify a complete recording through the configured ASR and cleanup endpoints, snippet expansion, History persistence, and automatic paste. For UI changes, inspect the actual Electron window. Test cloud credentials only against endpoints the user selected. Remove disposable recordings, screenshots, profiles, and databases after verification.
