# whisper

A reduced desktop build of [OpenWhispr](https://github.com/OpenWhispr/openwhispr), based on version 1.10.2 at commit `834a0771`.

## Install

Requires an Apple Silicon Mac running macOS 12 Monterey or later.

```sh
brew install --cask softmaxe/tap/whisper
```

Alternatively, download the ARM64 ZIP from [GitHub Releases](https://github.com/softmaxe/whisper/releases/latest), extract it, and move `whisper.app` to Applications. Each release includes a SHA-256 checksum file. Release applications use ad-hoc signing and are not notarized.

To update a Homebrew installation:

```sh
brew update
brew upgrade --cask softmaxe/tap/whisper
```

## Scope

- Global dictation hotkey, recording pill, microphone selection, and automatic paste.
- Self-hosted ASR with a custom server URL and model name.
- Self-hosted text cleanup with the original Prompt Studio for viewing, editing, resetting, and testing prompts.
- Dictionary, automatic correction learning, import/export, and Snippets.
- Local History, transcript search, copy/delete, audio retention, and retry.
- Local Insights with the original dictation metrics and activity heatmap.
- Single-file and batch Upload through the configured self-hosted ASR server, saved to History.
- A persistent macOS menu bar icon toggle in Preferences, using the original tray icon and menu.
- Original window layout, components, icons, colors, theme controls, and translations.

Accounts, billing, advertising, cloud sync, meetings, notes, assistant actions, translation mode, local model downloads, and upstream automatic updates are not exposed or started. The original recorder, model request code, SQLite database, dictionary logic, snippet expansion, and native keyboard/paste helpers are reused. Shared internal helpers remain where those implementations depend on them.

## Fonts

English text uses the bundled public JetBrains Mono webfonts. Chinese uses system PingFang on macOS, with platform fallbacks elsewhere. Font files and the OFL-1.1 license are in `src/assets/fonts/jetbrains-mono/`. The pinned source revision is recorded there. System fonts are not redistributed, and builds need no private font repository.

## Development

Use Node.js 24, as pinned in `.nvmrc`.

```sh
npm ci
npm run dev
```

Build a local application bundle:

```sh
npm run pack
```

On Apple Silicon the result is `dist/mac-arm64/whisper.app`. The macOS bundle uses ad-hoc signing for local use. It is not notarized for public distribution.

This build has a separate application identifier and stores its data in `~/Library/Application Support/whisper/` on macOS. It does not overwrite the installed original application's data. Quit the original application before using the same dictation shortcut in this build.

Configure the ASR server in Settings > Speech-to-Text and the cleanup server in Settings > Language Models. The endpoints may be localhost, a LAN address, or a remote server. The URL/model panels and request implementations are the upstream self-hosted components.

Upload accepts local audio/video files and batches through the original picker, drop target, format conversion, and queue. It shares the self-hosted endpoint and model with dictation. Results use the existing History storage instead of Notes. Upload keeps the upstream ASR-only behavior: it does not apply the dictation cleanup prompt or expand snippets, and does not retain a copy of the source audio. When local History is disabled, results remain available for copying without being saved. URL import and speaker detection are excluded.

Insights measures dictation on this device. Imported files do not count as dictation activity. Account sync and leaderboards are excluded. The original self-hosted upload cancellation behavior is retained: cancelling discards a late result, while an in-flight server request may continue. The upstream self-hosted upload path also sends no API-key header.

## Verification

```sh
npm run lint
npm run typecheck
npm test
```

Tests for retired features were removed or narrowed along with their features. Shared recorder, routing, dictionary, snippets, clipboard, and UI tests remain. Native SQLite tests must use a runtime matching the installed native binding. After `npm ci` or packaging, the binding targets Electron; run the tests with:

```sh
ELECTRON_RUN_AS_NODE=1 node_modules/.bin/electron --import tsx --test 'test/**/*.test.js'
```

The full Electron test run passed 4,459 tests, with 12 skipped and one existing TODO. Lint, type checking, the macOS build, and signature verification passed.

## Local acceptance checks

Verified on macOS with the existing user's Qwen/Qwen3-ASR-1.7B ASR endpoint, qwen3.5:9b cleanup endpoint, saved custom cleanup prompt, dictionary, and snippets:

- A recorded speech fixture reached ASR and cleanup successfully, expanded both existing snippet triggers, and saved raw and cleaned text to History.
- Dictionary and snippet creation, editing, deletion, and SQLite persistence worked.
- Single-file and batch uploads reached the configured ASR endpoint, including AIFF conversion, and saved their raw results to History. The batch continued while viewing Insights.
- Insights refreshed from local events and excluded imported files from dictation metrics.
- The macOS menu bar icon appeared and disappeared when toggled in Preferences. The disabled state survived an application restart; re-enabling restored the original menu.
- Microphone and accessibility checks succeeded, and automatic paste wrote the expected text into a disposable TextEdit document.
- Prompt Studio sent the saved custom prompt to the configured model. Ordinary Chinese and mixed-language checks succeeded. A translation/instruction stress case produced English from Chinese input, and an English recording partly became Chinese. The existing model behavior, upstream prompt suffixes, and user prompt were intentionally retained.

Early CDP reload checks encountered native Electron exits during a session that included duplicate packaged instances. Subsequent single-instance reloads and complete dictation/paste checks passed; the exact cause of those early exits was not established.

No credentials, private endpoint addresses, personal prompt text, or recorded audio are included in the repository.

## License

The upstream code remains MIT-licensed. See `LICENSE`. JetBrains Mono is covered by its bundled OFL-1.1 license.

## Releases

This repository starts its own version history at `1.0.0`. The upstream license and source attribution are retained.

The GitHub Actions release workflow follows the ARM64 app archive and tap-update pattern used by [quota-bar](https://github.com/softmaxe/quota-bar). A `vX.Y.Z` tag must match both package manifests. The workflow tests and packages the app, verifies its signature and architecture, publishes the ZIP and checksum, then updates `Casks/whisper.rb` in [softmaxe/homebrew-tap](https://github.com/softmaxe/homebrew-tap).

Release publishing uses the built-in GitHub token. Cross-repository tap updates use the `TAP_GITHUB_TOKEN` Actions secret, scoped to write the tap repository. Credentials belong in GitHub secrets and are never included in app bundles.

## Local setup artifacts

Node.js 24 was installed through Homebrew without changing the default Node.js executable. It can be removed with `brew uninstall node@24`. Application data is in `~/Library/Application Support/whisper/`; remove it only when intentionally resetting this application.
