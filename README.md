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

Or download the ARM64 ZIP from [GitHub Releases](https://github.com/softmaxe/whisper/releases/latest) and move `Whisper.app` to Applications. Releases include SHA-256 checksums. Release builds use a persistent self-signed certificate and are not notarized.

To update:

```sh
brew update
brew upgrade --cask softmaxe/tap/whisper
```

The first upgrade from an older ad-hoc signed build may require microphone, Accessibility, and Keychain access again. Later releases keep the same signing identity so macOS can recognize the app across updates. This does not remove Gatekeeper's warning about an unidentified developer. See [macOS signing](docs/macos-signing.md).

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

Run `npm run pack` to build an ad-hoc signed development app at `dist/mac-arm64/Whisper.app`. Use `npm run pack:release` for a release build with the persistent signing identity. Release signing requires the saved signing credentials and refuses to fall back to ad-hoc signing. See [macOS signing](docs/macos-signing.md) for setup and backup instructions.

Run the checks after installing dependencies:

```sh
npm run quality-check
```

This runs lint, TypeScript, translation checks, and the [Whisper regression suite](test/README.md). Run `npm test` for the tests alone. Tests use Electron's Node runtime to match the SQLite binding installed by `npm ci`; a missing binding fails the suite instead of skipping database tests.

The `CI` workflow runs these checks for pull requests targeting `main` and pushes to `main`. It does not package or upload the app. Run the `Build` workflow manually to check and package an ad-hoc signed macOS ARM64 app, available as an Actions artifact for seven days. Pushing a `v*` tag starts the `Release` workflow, which calls `Build` with the persistent signing identity, verifies it against the pinned public certificate, publishes the package to this repository, and updates `softmaxe/homebrew-tap`. Manual builds do not publish a release or update Homebrew.

## License

[MIT](LICENSE), with the upstream OpenWhispr attribution retained. Bundled JetBrains Mono fonts use [OFL-1.1](src/assets/fonts/jetbrains-mono/OFL.txt).
