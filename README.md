<p align="center">
  <img src="src/assets/icon.png" alt="Whisper app icon" width="128">
</p>

<h1 align="center">Whisper</h1>

<p align="center">
  <a href="README.md"><kbd>English</kbd></a>
  <a href="README.zh-CN.md"><kbd>简体中文</kbd></a>
</p>

A macOS dictation app for your own speech recognition and text cleanup servers. Speak, and Whisper pastes the text into your current app. Deploy the servers separately and connect them in Settings.

<p align="center">
  <img src="docs/images/overview-en.png" alt="Whisper interface with local dictation history and navigation" width="800">
</p>

The interface above uses sample data.

## Features

- Dictate with a global shortcut, a recording pill, microphone selection, and automatic paste.
- Connect self-hosted speech recognition and text cleanup services, with editable cleanup prompts.
- Keep a custom dictionary, learn from corrections, and expand spoken shortcuts with Snippets.
- Search, copy, delete, and retry local History entries, with configurable audio retention.
- Transcribe audio and video files, individually or in batches.
- View local dictation Insights and choose whether to keep the menu bar icon visible.

This fork focuses on dictation and file transcription. It does not include OpenWhispr Cloud, accounts, sync, meetings, Notes, AI Assistant, or bundled model servers.

## Install

Native builds require an Apple Silicon Mac with macOS 27 or later. This branch is
undergoing [native migration acceptance](docs/native-migration-status.md); the
commands below install the latest published release. Keep the existing daily-use
app until native acceptance and a separate release are complete.

```sh
brew install --cask softmaxe/tap/whisper
```

Or download the ARM64 ZIP from [GitHub Releases](https://github.com/softmaxe/whisper/releases/latest) and move `Whisper.app` to `/Applications`. Each release includes a SHA-256 checksum.

To update:

```sh
brew update
brew upgrade --cask softmaxe/tap/whisper
```

Releases use a persistent self-signed certificate and are not notarized. macOS may warn on first launch; upgrading from an older ad-hoc build may require permissions again. See [macOS signing](docs/macos-signing.md).

## Quick start

1. Open **Settings → Speech-to-Text** and enter your ASR server URL and model. The server must support an OpenAI-compatible `/audio/transcriptions` endpoint. Include `/v1` in the URL if your server requires it.
2. Configure your text cleanup server and prompt in **Settings → Text cleanup**. It uses `/v1/chat/completions`.
3. Grant microphone access for recording and Accessibility access for automatic paste. Choose your shortcut in **Settings → Hotkeys**.
4. Focus a text field and use the shortcut to dictate. To transcribe existing files, open **Upload**.

Upload uses the same ASR settings and saves raw transcripts to History. It skips text cleanup and Snippets, does not retain source audio, and does not count toward Insights. Results remain copyable when History is disabled.

Processing stays on your Mac only when your servers run locally. Local and private-network hosts can use HTTP; public hosts require HTTPS. See [Data and permissions](docs/data-and-permissions.md), or use the [custom ASR shim](examples/custom-asr-shim/) for other server APIs.

## Development

Use Xcode 27 with the macOS 27 SDK and Node.js 24 from [`.nvmrc`](.nvmrc).
Node.js is a build/test tool; native packages contain no Electron, Chromium or
Node.js runtime.

```sh
brew install pkgconf
npm ci
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:test
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run pack
```

`npm run build` builds native Swift code. `npm run pack` produces a credential-free,
ad-hoc native development bundle and archive. `npm run pack:release` uses the
original signing identity and fails if it is unavailable or mismatched. Both
commands verify arm64, macOS 27, native runtime dependencies, archive contents and
checksum without installing anything. Use the signed bundle with a disposable
`--profile` for manual development; see [native development](native/README.md).

Run `npm run quality-check` for retained legacy lint, TypeScript, translation and
regression coverage, plus `npm run native:test` for native workflows. Explicit
`legacy:dev`, `legacy:pack`, and `legacy:pack:release` commands preserve the legacy
comparison workflow. Packaging does not delete existing baseline archives. See
[Tests](test/README.md) and [macOS signing](docs/macos-signing.md).

## License

[MIT](LICENSE). Forked from [OpenWhispr](https://github.com/OpenWhispr/openwhispr) 1.10.2 at commit `834a0771`, with its attribution retained. Bundled JetBrains Mono fonts use [OFL-1.1](src/assets/fonts/jetbrains-mono/OFL.txt).

Native archives include the standalone [FFmpeg](https://ffmpeg.org/) executable
under LGPL 2.1 or later and [LAME](https://lame.sourceforge.io/) under LGPL 2.0 or
later. Exact source archives, licenses and rebuild instructions are included in
`Whisper.app/Contents/Resources/licenses/ffmpeg` alongside the distributed binary.
