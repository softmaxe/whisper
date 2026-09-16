# Assets

Application artwork and provider icons come from OpenWhispr.

- `icon.icns` is the macOS app icon.
- `openwhispr.icon/` is the Icon Composer source for `scripts/compile-macos-icon.js`.
- `iconTemplate@3x.png` is the macOS menu bar template icon.
- `icon.png` is the favicon and fallback icon. The tray helper also references `icon.ico`.
- `icons/providers/` supplies `src/utils/providerIcons.ts`.
- [JetBrains Mono](fonts/jetbrains-mono/) provides Latin fonts and their OFL-1.1 license. Chinese text uses system PingFang on macOS.
- `fonts/noto-sans.css` defines the bundled Noto Sans fallback loaded by `src/index.html`.

Preserve the upstream attribution and bundled font licenses when changing these assets.
