# Troubleshooting

## Recording and paste

- Check the selected microphone in Preferences and confirm that it captures sound.
- Enable Whisper in macOS System Settings > Privacy & Security > Microphone.
- Enable Whisper under Accessibility for automatic paste and native shortcuts. Restart if permission changes are not detected.
- Quit the original OpenWhispr app when both apps use the same dictation shortcut.
- If paste fails, check the focused text field and copy the result from History.

## ASR or Clean Up fails

Check the URL and model in Settings > Speech-to-Text or Settings > Language Models. Confirm the selected server is reachable and accepts the configured credentials.

Whisper does not fall back to another provider. Inspect [network errors](docs/network-allowlist.md) and [debug logs](DEBUG.md).

Use Prompt Studio to test the saved Clean Up prompt when the server responds but the output is unexpected.

## Upload

Upload uses the dictation ASR endpoint and model. Its transport sends no API-key header. When History is disabled, copy successful results from Upload.

For conversion failures, check the FFmpeg error in the debug log. Packaged builds include FFmpeg; `npm ci` installs it for development. Cancellation discards the result but cannot stop a request already running on the server.

## Development and native modules

Use Node.js from `.nvmrc`. Both `uname -m` and `node -p "process.arch"` should report `arm64`.

If Electron's binary is missing after `npm ci`, enable install scripts and Electron downloads, then run `npm rebuild electron`.

A SQLite ABI mismatch means the runtime and native binding differ. Use the Electron test command in [README.md](README.md#development).

## Reporting a problem

Open an issue in [softmaxe/whisper](https://github.com/softmaxe/whisper/issues) with the app and macOS versions, reproduction steps, expected behavior, and redacted errors.

`~/Library/Application Support/whisper/` contains settings and local data. Do not delete it as a routine troubleshooting step.
