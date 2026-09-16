# Whisper

Whisper is a stripped-down fork of [OpenWhispr](https://github.com/OpenWhispr/openwhispr) for local deployment on macOS. It keeps desktop dictation and uses self-hosted services for speech recognition and text cleanup. Deploy those services separately, then configure their URLs and model names in the app.

Based on OpenWhispr 1.10.2 at commit `834a0771`.

## Scope

- Global dictation shortcut, recording pill, microphone selection, and automatic paste.
- Self-hosted ASR and text cleanup with editable prompts.
- Dictionary, correction learning, and Snippets.
- Local History with search, copy, delete, audio retention, and retry.
- Local audio/video transcription, including batch uploads.
- Local dictation Insights and a persistent menu bar icon toggle.

Accounts, billing, advertising, cloud sync, meetings, Notes, AI Assistant, translation mode, model downloads, URL imports, speaker detection, and in-app automatic updates are excluded.

Upload shares the dictation ASR configuration. It saves raw transcripts to History without text cleanup, snippet expansion, or source audio retention. Results remain copyable when History is disabled. Uploads do not count toward dictation Insights.

## Install

Requires an Apple Silicon Mac running macOS 12 Monterey or later.

```sh
brew install --cask softmaxe/tap/whisper
```

Or download the ARM64 ZIP from [GitHub Releases](https://github.com/softmaxe/whisper/releases/latest) and move `Whisper.app` to Applications. Releases include SHA-256 checksums. The app uses ad-hoc signing and is not notarized.

To update:

```sh
brew update
brew upgrade --cask softmaxe/tap/whisper
```

## Configuration

Set the ASR URL and model in Settings > Speech-to-Text. Configure text cleanup in Settings > Language Models. Both support localhost, LAN, and remote servers. For ASR servers that need an adapter, see the [custom ASR shim](examples/custom-asr-shim/).

Grant microphone access for recording and Accessibility access for automatic paste. App data is stored in `~/Library/Application Support/whisper/`, separately from OpenWhispr.

## Development

Use Node.js 24 from `.nvmrc`.

```sh
npm ci
npm run dev
```

Run `npm run pack` to build `dist/mac-arm64/Whisper.app`.

Run the checks with:

```sh
npm run lint
npm run typecheck
npm run i18n:check
npm test
```

After `npm ci` or packaging, the SQLite binding targets Electron. Use the Electron runtime instead of `npm test`:

```sh
ELECTRON_RUN_AS_NODE=1 node_modules/.bin/electron --import tsx --test 'test/**/*.test.js'
```

## License

[MIT](LICENSE), with the upstream OpenWhispr attribution retained. Bundled JetBrains Mono fonts use [OFL-1.1](src/assets/fonts/jetbrains-mono/OFL.txt).
