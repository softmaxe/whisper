# Native macOS application

This Swift application develops alongside the existing Electron application. It
targets Apple Silicon and macOS 27. The initial slice implements Settings;
Dictation and the remaining pages follow through specification #35.

Use Xcode 27 per command without changing the global Command Line Tools selection:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:build
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:test
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer npm run native:pack
```

Use Node.js 24 and `npm ci` for repository scripts. The Swift package has no external
dependencies. Node.js is only a build tool; the app contains Swift code, system
frameworks, app resources, and no Web runtime. Packaging requires the original
[signing identity](../docs/macos-signing.md) and verifies the pinned certificate,
bundle identifier, architecture, minimum OS, and runtime dependencies. It creates
`dist/native-arm64/Whisper.app` without installing or launching it.

The native app uses `~/Library/Application Support/WhisperNative` and a separate
Keychain service, `local.whisper.desktop.native`. It never reads the legacy profile
or imports its credentials. ASR secrets are stored only in Keychain, while the
profile holds an opaque account reference. A failed credential write does not
replace the saved settings, and an unreadable profile cannot be overwritten by a
save. The app supports `--profile /absolute/path/to/isolated-profile` for disposable
manual tests. This option does not change the default profile.

`WhisperApplication.send` is the shared command boundary for UI and native adapters.
Views observe its `state`. Tests exercise these same commands and reopen actual
temporary profiles. Credential persistence tests use real, temporary file
Keychains, deleted with their fixture directories. They do not use the login
Keychain or private user data. The deprecated file-Keychain creation API is confined
to test setup so each test can isolate its storage; application access uses SecItem.

The Settings structure, English/Chinese copy, and endpoint policy adapt the existing
implementation and OpenWhispr at `6d56d75e7e13ec47009e573e9ff4cded0d0ccc61`:
`src/components/SelfHostedPanel.tsx`, `src/components/SettingsPage.tsx`, and
`src/utils/urlUtils.ts`. The Keychain contract follows `src/helpers/secretCrypto.js`
without its Electron encryption dependency. Native text uses the existing design's
JetBrains Mono and the system Chinese fallback. The bundled TTF faces are from the
[same JetBrains Mono revision](https://github.com/JetBrains/JetBrainsMono/tree/19371302b95d218af43299bce79ddbddd0bc364d)
as the legacy webfonts, with their OFL-1.1 license retained in the resource bundle.

A successful package or deterministic test does not establish hardware microphone,
Accessibility, or Keychain permission retention after a Homebrew upgrade. Follow
[release smoke checks](../test/README.md#release-smoke-check) before making those
claims. Do not replace the daily-use app during this staged implementation.
