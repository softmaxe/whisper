# Working on Whisper

Respond in Chinese. Write code comments, documentation, configuration comments, commit messages, branch names, and PR text in English.

Keep project instructions here and update them when the agreed scope changes. Do not create a parallel `CLAUDE.md`. Documentation should cover current project behavior and workflows; omit personal preferences, machine setup history, and session logs.

## Scope and implementation

Read the [supported scope](README.md#scope) before changing behavior. Preserve Dictionary, Snippets, History, shortcuts, microphone selection, the recording pill, and automatic paste.

- Trace the existing UI, renderer service, IPC handler, and storage or native helper. Check the retained OpenWhispr implementation and tests before replacing code. Reuse or adapt working paths; explain why reuse is insufficient and ask before taking a new approach outside the authorized scope.
- Ask before adding, removing, or restoring features when scope is unclear. Dormant modules, dependencies, and tests do not authorize restoring excluded features or removing all shared code that mentions them. Routine work already authorized by the user can proceed.
- ASR and Clean Up use Self-Hosted only. Preserve custom localhost, LAN, and remote URLs, model names, supported credentials, saved prompts, and upstream language suffixes. Report language drift or request failures without silently changing prompts or falling back to another provider.
- Upload reuses the dictation ASR endpoint and model for local audio/video files and batches. Save raw text to History with `routeKind: "upload"`; do not apply cleanup, expand snippets, or retain source audio. Keep successful results copyable when History is disabled.
- Upload continues across navigation. Cancellation discards late results because the existing transport cannot abort in-flight requests; retain that transport. Exclude uploads from dictation counts, speed, and activity in Insights.
- The menu bar toggle reuses the original tray icon and menu, defaults to visible, applies immediately, and persists across restarts.

## UI and application identity

- Reuse upstream components, spacing, colors, icons, and interactions. Ask before UI changes outside the requested scope. Use existing i18n keys where suitable and add approved new copy to every supported locale.
- Use bundled public JetBrains Mono for English and system PingFang for Chinese on macOS. Preserve font licenses and source attribution in `src/assets/fonts/jetbrains-mono/`; builds must not require private fonts.
- Read `electron-builder.json` for bundle naming. The display name is `Whisper` and the bundle is `Whisper.app`; package, repository, cask, and archive names use `whisper`. Match verification and cask paths to the build configuration.
- Preserve `local.whisper.desktop`, the `whisper` Keychain namespace, and `~/Library/Application Support/whisper/`. Display-name changes do not migrate data. Keep this fork's data and cleanup separate from OpenWhispr and shared caches.

## Code locations

| Behavior | Start here |
| --- | --- |
| Dictation and paste | `src/hooks/useAudioRecording.js`, `src/helpers/audioManager.js`, `src/helpers/clipboard.js` |
| Cleanup and prompts | `src/services/ReasoningService.ts`, `src/config/prompts/` |
| Dictionary, Snippets, History | `src/components/DictionaryView.tsx`, `src/components/SnippetsView.tsx`, `src/helpers/database.js` |
| Upload | `src/components/notes/UploadAudioView.tsx`, `src/services/fileTranscription.ts`, `src/services/uploadNotes.ts`, `src/stores/batchQueueStore.ts` |
| Insights | `src/components/InsightsView.tsx`, `src/helpers/analytics.js`, `src/helpers/database.js` |
| Preferences and menu bar | `src/components/SettingsPage.tsx`, `src/stores/settingsStore.ts`, `src/helpers/environment.js`, `src/helpers/tray.js`, `main.js` |
| IPC | `preload.js`, `src/helpers/ipcHandlers.js`, `src/types/electron.ts` |

Update both IPC sides and their types together. `uploadNotes.ts` saves to History; its name does not imply Notes support.

## Verification and cleanup

- Use `.nvmrc` and existing npm scripts. Keep lockfile changes deliberate. For documentation-only edits or removal of dependency-update automation, inspect documents and diffs; skip tests, quality checks, dependency installation, and bundles.
- For behavior changes, run relevant tests and applicable lint, type, and translation checks. Use the [Electron test command](README.md#development) when SQLite bindings target Electron. An ABI mismatch does not establish a behavior failure or success.
- Verify dictation ASR, the saved cleanup prompt, snippets, History, and paste together. Check single and batch Upload, History-disabled copying, and exclusion from Insights. Check menu bar visibility immediately and after restart. Inspect UI changes in the actual Electron app using the current bundle path.
- When endpoint testing is authorized, use the selected Self-Hosted configuration and prompt with disposable fixtures and `OPENWHISPR_USER_DATA_DIR`. Keep private URLs, credentials, prompts, recordings, and copied profiles out of source, public logs, and releases.
- Use existing authorization for routine permission handling. Identify any macOS action only the user can perform and continue independent work.
- Remove task-created profiles, recordings, screenshots, and temporary files. Before deleting build output, check tracked files and running processes. Preserve personal settings, Dictionary, Snippets, History, unrelated shared caches, and system packages.

## Git and releases

- Check branch, status, remotes, and contribution rules before editing. Preserve others' changes. Commit, push, tag, publish, merge, or rewrite history only when requested; each release needs authorization. Stage explicit paths and omit AI attribution from commits and PRs.
- Keep dependency updates manual; do not enable Dependabot version-update automation. Preserve the upstream MIT license, source attribution, and font licenses.
- For releases, read versions from both package manifests and use `.github/workflows/build.yml` and `.github/workflows/release.yml`. Tags must match the manifests. Verify the packaged version, arm64 architecture, signature, ZIP, and checksum before publication. Builds use ad-hoc signing and are not notarized.
- Follow the release flow from `softmaxe/whisper` to `softmaxe/homebrew-tap/Casks/whisper.rb`. Keep `TAP_GITHUB_TOKEN` in Actions secrets. Do not bypass failed checks or overwrite published assets. Completion requires successful workflows and tap download/checksum verification.
