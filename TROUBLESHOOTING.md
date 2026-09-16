# Troubleshooting

This build supports macOS on Apple Silicon and uses the configured Self-Hosted servers for ASR and Clean Up. See [README.md](README.md) for installation, configuration, and the supported scope.

## Recording and paste

- Check the selected microphone in Preferences and confirm that it captures sound.
- Enable Whisper in macOS System Settings > Privacy & Security > Microphone.
- For automatic paste and native shortcuts, enable Whisper under Accessibility. Restart the app after changing permissions if the change is not detected.
- Quit the original OpenWhispr app when both apps use the same dictation shortcut.
- If transcription succeeds but paste fails, copy the result from History and check Accessibility permissions and the focused text field.

## ASR or Clean Up fails

Check the server URL and model in Settings > Speech-to-Text or Settings > Language Models. Confirm the server is running and reachable from this Mac. Localhost, LAN, and remote URLs are supported.

Check the selected endpoint's supported credentials and inspect the request error. Whisper does not fall back to another provider. See [Network access](docs/network-allowlist.md) and [Debug logging](DEBUG.md).

Prompt Studio can test the saved Clean Up prompt. Known language-switching cases are recorded in the README; changing the prompt changes the resulting behavior.

## Upload

Upload shares the dictation ASR endpoint and model. It saves raw ASR text to History without Clean Up or snippet expansion. When History is disabled, successful results remain copyable in Upload.

For conversion errors, check the FFmpeg error in the debug log. Packaged builds include FFmpeg; source builds install it through `npm ci`. A cancelled Self-Hosted request may continue on the server, but its late result is discarded. The upstream upload transport sends no API-key header.

## Development and native modules

Use the Node.js version in `.nvmrc`. On Apple Silicon, `uname -m` should report `arm64` and `node -p "process.arch"` should report `arm64`.

Install dependencies with `npm ci`; keep the tracked lockfile. If Electron's binary is missing, check that install scripts and Electron downloads are enabled, then run `npm rebuild electron`.

A SQLite ABI mismatch means the runtime and native binding do not match. After installation or packaging, run the native tests using the Electron command in the README.

## Reporting a problem

Open an issue in [softmaxe/whisper](https://github.com/softmaxe/whisper/issues) with the app version, macOS version, reproduction steps, expected behavior, and redacted errors. See [DEBUG.md](DEBUG.md) for log locations.

Application data is stored in `~/Library/Application Support/whisper/`. Do not delete this directory or shared OpenWhispr caches as a routine troubleshooting step; it contains personal settings and local data.
