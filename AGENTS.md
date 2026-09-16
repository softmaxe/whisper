# Working on Whisper

Keep repository-specific agent instructions in this file. Update it when the user changes the agreed scope; do not maintain a parallel `CLAUDE.md`.

Respond in Chinese. Write code comments, documentation, configuration comments, commit messages, branch names, and PR text in English. Keep technical identifiers and commands unchanged.

## Reuse before implementation

- Trace the existing UI, renderer service, IPC handler, and storage or native helper before changing a behavior. Check the retained OpenWhispr implementation and its tests before proposing a replacement.
- Prefer reconnecting, narrowing, or adapting proven code. New implementation is a last resort when an existing path cannot run or cannot be separated from excluded features. Explain what can be reused and why the remaining change is necessary; ask the user before taking an unapproved new approach.
- Ask the user when it is unclear whether to keep, remove, or add a feature. Also ask before introducing UI copy or changing colors, layout, typography, or interaction patterns outside the requested change.
- Proceed with routine work already authorized by the request, including reuse of existing components and styles. Apply decisions already made in the conversation without requesting the same approval again.
- Retained modules, dependencies, scripts, and tests may support dormant upstream features. Their presence does not authorize re-enabling those features or justify a broad rewrite to remove every reference.

## Supported behavior

Read the Scope and Local acceptance checks sections of [README.md](README.md) before changing behavior. Preserve Dictionary, Snippets, History, and the original dictation infrastructure: shortcuts, microphone selection, recording pill, and automatic paste.

- ASR and Clean Up use Self-Hosted only. Preserve arbitrary custom URLs, including localhost, LAN, and remote servers, along with model names, supported credentials, and saved prompts. Never fall back silently to another provider.
- Preserve the user's Clean Up prompt and the upstream language suffix logic. The known language-switching cases in the README were explicitly accepted; record failures without silently rewriting the prompt or changing model behavior.
- Upload supports local audio/video files and batches through the original transcription path. It shares the dictation ASR endpoint and model, saves ASR text to History with `routeKind: "upload"`, and does not apply dictation cleanup or snippet expansion. It does not retain source audio. When History is disabled, successful results remain copyable without being saved.
- Preserve upload continuation across navigation. The original Self-Hosted transport cannot abort an in-flight request; cancellation discards its late result. Do not replace that transport merely to change cancellation behavior.
- Insights reports local dictation metrics and activity. Uploaded files must remain excluded from dictation counts, speed, and activity.
- The macOS menu bar toggle reuses the original tray icon and menu. It defaults to visible, changes visibility immediately, and persists across restarts.

Accounts, billing, promotions, cloud sync, meetings, Notes, AI Assistant, translation mode, local model downloads, URL imports, speaker detection, and in-app automatic updates remain outside the agreed scope. Ask before restoring any of them.

## UI and application identity

- Reuse the original components, spacing, colors, icons, and interaction patterns. Keep UI changes as close to upstream as the approved scope permits. Use existing i18n keys where they fit; approved new copy needs matching keys in every supported locale.
- English uses the bundled public JetBrains Mono fonts; Chinese uses system PingFang on macOS. Keep the font license and source attribution in `src/assets/fonts/jetbrains-mono/`. Existing SVG icons remove the need for a Nerd Font dependency. Builds must not depend on private fonts or font repositories.
- Read `electron-builder.json` for display and bundle naming. The current display name is `Whisper` and the bundle is `Whisper.app`; the package, repository, Homebrew cask, and archive prefix remain lowercase `whisper`. Match build verification and cask bundle paths to the packaging configuration rather than applying a global text replacement.
- Preserve `local.whisper.desktop`, the lowercase `whisper` Keychain namespace, and `~/Library/Application Support/whisper/`. A display-name change is not a data migration. Keep the fork's data and cleanup operations separate from the original OpenWhispr installation and shared caches.

## Find the existing implementation

| Change | Start here |
| --- | --- |
| Dictation and paste | `src/hooks/useAudioRecording.js`, `src/helpers/audioManager.js`, `src/helpers/clipboard.js` |
| Cleanup and prompts | `src/services/ReasoningService.ts`, `src/config/prompts/` |
| Dictionary, snippets, and History | `src/components/DictionaryView.tsx`, `src/components/SnippetsView.tsx`, `src/helpers/database.js` |
| Upload | `src/components/notes/UploadAudioView.tsx`, `src/services/fileTranscription.ts`, `src/services/uploadNotes.ts`, `src/stores/batchQueueStore.ts` |
| Insights | `src/components/InsightsView.tsx`, `src/helpers/analytics.js`, `src/helpers/database.js` |
| Preferences and menu bar | `src/components/SettingsPage.tsx`, `src/stores/settingsStore.ts`, `src/helpers/environment.js`, `src/helpers/tray.js`, `main.js` |
| Renderer/main boundary | `preload.js`, `src/helpers/ipcHandlers.js`, `src/types/electron.ts` |

Update both sides of an IPC change and their types. The legacy filename `uploadNotes.ts` now contains the History save path; it is not a reason to restore Notes.

## Verification and cleanup

- Use the Node.js version in `.nvmrc` and the existing npm scripts. Keep lockfile changes deliberate. For documentation-only work or removal of dependency-update automation, inspect the documentation and diff; skip tests, quality checks, dependency installation, and app bundles.
- Run checks appropriate to the behavior changed, then the applicable lint, type, and translation checks. Use the native Electron test command in the README when SQLite bindings target Electron. A Node ABI mismatch is not evidence that database behavior is correct or broken.
- For dictation changes, verify ASR, the saved Clean Up prompt, snippets, History, and paste together. For Upload, verify single and batch results, History-disabled behavior, and exclusion from Insights. For the menu bar, verify immediate visibility and restart persistence. Inspect UI changes in the actual Electron app, using the current bundle path.
- When real endpoint testing is authorized, use the user's existing selected Self-Hosted configuration and prompt. Use disposable fixtures and the existing `OPENWHISPR_USER_DATA_DIR` override for test profiles. Keep private URLs, credentials, prompt text, recordings, and copied profiles out of source, public logs, and releases.
- Use existing authorization for routine permission handling when the tools can perform it. If macOS requires an action only the user can complete, identify that exact requirement and continue independent work.
- Remove task-created test profiles, recordings, screenshots, and temporary files after verification. A cleanup request for build artifacts does not authorize deleting personal settings, Dictionary, Snippets, or History. Check tracked files and running processes before removing generated directories; leave unrelated shared caches and system packages alone.

## Git and releases

- Keep dependency updates manual. Dependabot version-update PR automation is outside this independent project's scope.
- Check branch, status, remotes, and contribution rules before editing. Preserve changes made by the user or another task. Commit, push, tag, publish, merge, or rewrite history only when requested; a prior completed release is not standing authorization for the next one. Stage explicit paths and keep commit and PR text free of AI attribution.
- Preserve the upstream MIT license, source attribution, and bundled font license. This fork has its own release history; read current versions from the manifests rather than treating an upstream version as the fork's version.
- When release work is requested, reuse `.github/workflows/build.yml` and `.github/workflows/release.yml`. The supported distribution is macOS Apple Silicon. Tags must match both package manifests; verify the packaged version, arm64 architecture, signature, ZIP, and checksum before publication. Existing builds use ad-hoc signing and are not notarized.
- Follow the established Release-to-Homebrew flow for `softmaxe/whisper` and `softmaxe/homebrew-tap/Casks/whisper.rb`. Keep `TAP_GITHUB_TOKEN` in Actions secrets. Do not bypass failed checks, overwrite published assets, or report completion before the workflow and tap download/checksum verification succeed.
