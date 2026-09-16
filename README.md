# Whisper

Whisper is a macOS dictation app that connects to your own speech recognition and text cleanup servers. Deploy those services separately, then configure their URLs and model names in the app.

Forked from [OpenWhispr](https://github.com/OpenWhispr/openwhispr) 1.10.2 at commit `834a0771`.

## Scope

- Global dictation shortcut, recording pill, microphone selection, and automatic paste.
- Self-hosted ASR and text cleanup with editable prompts.
- Dictionary, correction learning, and Snippets.
- Local History with search, copy, delete, audio retention, and retry.
- Audio/video file transcription, including batch uploads.
- Local dictation Insights and a persistent menu bar icon toggle.

This fork focuses on dictation and file transcription. It does not include OpenWhispr Cloud, accounts, sync, meetings, Notes, AI Assistant, or bundled model servers.

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

Set the ASR URL and model in Settings > Speech-to-Text. Configure text cleanup in Settings > Text cleanup. Use HTTP for local or private-network hosts and HTTPS for public hosts.

ASR expects an OpenAI-compatible `/audio/transcriptions` endpoint. Include `/v1` in the server URL if your server requires it. Text cleanup uses `/v1/chat/completions`. For ASR servers with a different API, see the [custom ASR shim](examples/custom-asr-shim/).

Grant microphone access for recording and Accessibility access for automatic paste. App data is stored in `~/Library/Application Support/whisper/`, separately from OpenWhispr. See [Data and permissions](docs/data-and-permissions.md) for details.

## Development

Use Node.js 24 from `.nvmrc`.

```sh
npm ci
npm run dev
```

Run `npm run pack` to build `dist/mac-arm64/Whisper.app`.

Run the checks after installing dependencies:

```sh
npm run lint
npm run typecheck
npm run i18n:check
ELECTRON_RUN_AS_NODE=1 node_modules/.bin/electron --import tsx --test 'test/**/*.test.js'
```

`npm ci` and packaging build the SQLite binding for Electron, so the test command above uses the same runtime. `npm test` uses Node and requires a SQLite binding built for that Node version.

## License

[MIT](LICENSE), with the upstream OpenWhispr attribution retained. Bundled JetBrains Mono fonts use [OFL-1.1](src/assets/fonts/jetbrains-mono/OFL.txt).
